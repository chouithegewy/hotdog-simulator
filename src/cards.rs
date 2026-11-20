// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Fully Loaded (kitchen_sim::cards) on the client: the round handed over to the card table, and the table itself.
//!
//! While the rules say the hotdogs play cards (`rules.cards`, which single player takes from the page's address and a
//! client online from the server's frames) the round stands down ([`at_table`]: arena.rs keeps its camera, movement,
//! guns, pickups and head-up display to itself, and tags.rs its tags), and this module keeps what the table looks like:
//!
//! * [`Seen`] is the table now and the seat this player sits at. Single player reads it every frame from its own
//!   simulation's table (`Feast::cards`); online it is the latest the server sent (`{type:'cards',table}`, network.rs),
//!   which the server sends when the table changes and once on joining. Whether there is a table is the rules' to
//!   say, never a message's: the view goes as soon as the rules do not say cards, and a table heard before the
//!   frame that says cards (a join's comes before its first frame) waits for it.
//! * [`Wishes`] is what this player asks of the table: a flip, a stop, a hotdog picked. Single player plays them at
//!   once on its own table; online network.rs sends them as `{type:'cards',action:'flip'|'stop'|'target',seat?}` and
//!   the server plays them on this player's seat (never one a message names).
//!
//! Single player shuffles every game afresh ([`fresh_seed`]); online the server shuffles.
//!
//! What is drawn of the table is made when it is first seen and goes, all of it, as soon as this player is no longer
//! at it:
//!
//! * **The table**: an oval of green felt on one leg on the microwave's top, RX across and RZ deep, its top TOP up,
//!   with the hotdogs standing round it where the simulation stands them (`feast::table_seats`).
//! * **The cards** lie face up in a row in front of each hotdog, a Napkin held lying apart, and the draw pile in the
//!   middle is as high as the cards left in it. Each card is printed in its colour with a picture of its food (or of
//!   what it does) and its value in its corners ([`card_face`], faces.rs). This player's own cards are those in front
//!   of its seat: they are not shown again on the screen. They are dealt again whenever the table changes (its
//!   `version`), and whenever who sits where does (this player's seat, the seats' names and colours, which online
//!   come with the frames and may come after the table).
//! * **The seat**: this player looks from its own seat, its eye EYE over the microwave's top and BACK behind its seat,
//!   at the table's middle, and can look about with the mouse while the pointer is captured, as far as LOOK.
//! * **The hotdogs** the round draws at their seats (arena.rs single player, network.rs online) are posed by
//!   [`seated`]: no gun in hand, turned towards whoever's go or pick it is, cheering when fully loaded or the winner,
//!   and slumped for a moment after a bust. This player's own is not drawn: its eye is in it.
//! * **On the screen**: the scoreboard in the top-left corner ([`rows`]: each seat's name, points, how it stands as an
//!   icon and its cards as chips of their colours with their values), the name tags over the heads, clear of it
//!   ([`clear_of`]), and the prompt along the bottom ([`prompt`]), in icons (icons.rs): the mouse buttons and what they
//!   do (flip a card, stop), whom it waits on, each seat's points when a hand is over, the winner.
//! * **Input**: on this player's go the left button flips and the right stops. Picking who gets an action card, the
//!   left button picks the hotdog looked at ([`seat_in_sight`]) while the pointer is captured or, with the pointer
//!   freed (Esc), the one whose row is clicked on the scoreboard. A click on the scoreboard never captures the pointer
//!   and never flips; a click that captures the pointer does nothing else (in a browser also after a lock the page lost
//!   unheard). The touch controls (touch.rs) show Flip and Stop:
//!   Flip is the left button (picking, it picks the hotdog looked at), Stop the right (picking, it reads Me and gives
//!   the card to this player itself: [`scope`]), and a drag looks round. Nothing is taken while the window does not
//!   have the keyboard (the page's chat box is open).
//! * **The tutorial** (tutorial.rs): a table the address asks for it at (`?tutorial`, natively `--tutorial`; it never
//!   starts by itself) is a guided first hand. [`Tutorial`] lays the lesson's table in place of the game's (`teach`), takes
//!   in what happens at it step by step, and says each step in a coach box over the prompt line (`coach_box`). K, or a
//!   click on its note with the pointer free (or by touch Stop, while the lesson asks for a flip), skips it.
use crate::arena::Game;
use crate::icons::{Icon, Icons};
use crate::maya::HotdogCamera;
use crate::player_model::Cues;
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    light::NotShadowCaster,
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};
use kitchen_sim::cards::{Card, Command, Kind, Status, Table, View};
use kitchen_sim::feast::Player;
use std::collections::HashMap;

/// The table: an oval of felt RX across the microwave's top and RZ deep (bigger than the first sketch's round one of
/// 0.078 m), its top TOP over the microwave's.
const RX: f32 = 0.11;
const RZ: f32 = 0.08;
const TOP: f32 = 0.075;
/// A card, across and along, and how thick, and how high the draw pile is a card.
const CARD: Vec2 = Vec2::new(0.016, 0.022);
const THICK: f32 = 0.0004;
const PILE_THICK: f32 = 0.0001;
/// This player's eye: EYE over the microwave's top and BACK further from the table than its seat (so that the hotdogs
/// beside it are not at its shoulders), seeing FOV degrees up and down, and looking about from the table's middle as
/// far as LOOK degrees (across, and up and down) with the mouse, at SENSITIVITY radians a pixel (the round's).
const EYE: f32 = 0.15;
const BACK: f32 = 0.04;
const FOV: f32 = 70.;
const LOOK: Vec2 = Vec2::new(70., 35.);
const SENSITIVITY: f32 = 0.0022;
/// The point the eye looks at: AIM over the middle of the felt, so that the hotdogs across the table are in the
/// picture to the tops of their heads, their name tags too, as well as the near edge of the table.
const AIM: f32 = 0.015;
/// A hotdog is in sight to be picked within SIGHT degrees of where this player looks; it is looked at about CHEST over
/// its feet, and its name tag is HEAD over them.
const SIGHT: f32 = 25.;
const CHEST: f32 = 0.1;
const HEAD: f32 = 0.2;
/// How long (seconds) a pointer lock asked of a browser may take to come before the table stops waiting for it.
const PATIENCE: f32 = 1.;
/// How fast a hotdog turns to whoever's go it is (the share of the way left it turns in a second is 1 - e^-TURN_RATE),
/// and how long a bust slumps it.
const TURN_RATE: f32 = 5.;
const SLUMP: f32 = 1.;
/// Pixels between a name tag and the head under it, and between the scoreboard and a tag moved below it.
const GAP: f32 = 3.;
const CLEAR: f32 = 6.;

pub struct CardsPlugin;
impl Plugin for CardsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Seen>()
            .init_resource::<Wishes>()
            .init_resource::<Poses>()
            .init_resource::<Shown>()
            .init_resource::<Tutorial>()
            .init_resource::<Icons>()
            .add_systems(Update, (play, follow, teach, pose, lay, coach_box, sit).chain().after(crate::arena::tick).before(crate::network::send))
            // The name tags, once the camera and the hotdogs have their places for the frame and before the screen is laid out.
            .add_systems(PostUpdate, label.after(bevy::transform::TransformSystems::Propagate).before(bevy::ui::UiSystems::Content));
    }
}

/// What the table looks like now (None while the rules do not say cards) and which seat is this player's: its place
/// among the round's players (single player's player 1, online the hotdog the welcome named).
///
/// It is written every frame (`follow`), so Bevy takes it as changed every frame: what is drawn from it is drawn again
/// when the table's `version` moves or who sits where changes (`Deal`), never on change detection.
#[derive(Resource, Default)]
pub struct Seen {
    pub view: Option<View>,
    pub me: Option<usize>,
    /// Online: the latest table the server sent, kept until the rules say there is a table to show it at. (The server
    /// sends one only while there is a table, and a new game's before its first frame: a new map's or a join's frame
    /// may come a moment after it.)
    heard: Option<View>,
}
impl Seen {
    /// Online: a table the server sent (network.rs).
    pub fn hear(&mut self, view: View) {
        self.heard = Some(view);
    }
}

/// What this player has asked of the table since the last frame, oldest first.
#[derive(Resource, Default)]
pub struct Wishes(pub Vec<Command>);

/// Whether this player is at the card table: the rules say cards and a round is under way (not between rounds in the
/// fridge, nor in the opening scene).
pub fn at_table(g: &Game) -> bool {
    g.feast.rules.cards && g.playing()
}

/// What the table waits on this player (at seat `me`) for: its go, to flip or stop, and its pick of who gets an action
/// card it drew. Neither between hands or once there is a winner.
pub fn asks(view: &View, me: Option<usize>) -> (bool, bool) {
    let Some(me) = me.filter(|_| view.winner.is_none() && !view.between) else {
        return (false, false);
    };
    let picking = view.choosing.is_some_and(|(seat, _)| seat == me);
    (view.turn == Some(me) && view.choosing.is_none(), picking)
}

/// A wish as the server takes it: `{type:'cards',action:'flip'|'stop'|'target'}`, with `seat`, the hotdog picked, for
/// a target only.
pub fn wish(command: Command) -> serde_json::Value {
    match command {
        Command::Flip => serde_json::json!({"type": "cards", "action": "flip"}),
        Command::Stop => serde_json::json!({"type": "cards", "action": "stop"}),
        Command::Target(seat) => serde_json::json!({"type": "cards", "action": "target", "seat": seat}),
    }
}

/// A seed for a single-player game's shuffles, different every game: the clock natively, and in a browser (where the
/// standard library has no clock) the page's own random numbers.
pub fn fresh_seed() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (cards_random() * 9_007_199_254_740_992.) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
    }
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "export function cards_random(){return Math.random()}
export function cards_locked(){return !!document.pointerLockElement}")]
extern "C" {
    fn cards_random() -> f64;
    fn cards_locked() -> bool;
}

/// Every frame, after the arena's tick: the seat this player sits at and the table as it is. Single player plays its
/// wishes on its own table first; online they wait for network.rs. Away from the table nothing is wished for, and
/// without the rules saying cards nothing is seen. The offline demo is only watched: every hotdog at its table is a
/// bot's, so no seat is this player's, and nothing is wished of it.
fn follow(mut g: ResMut<Game>, mut seen: ResMut<Seen>, mut wishes: ResMut<Wishes>) {
    if !at_table(&g) || g.demo {
        wishes.0.clear();
    }
    if !g.feast.rules.cards {
        (seen.view, seen.me) = (None, None);
        return;
    }
    let me = g.me();
    if g.network {
        seen.me = g.roster.iter().position(|p| p.id == me);
        if let Some(view) = seen.heard.take() {
            seen.view = Some(view);
        }
        return;
    }
    seen.me = g.feast.players.iter().position(|p| p.id == me).filter(|_| !g.demo);
    for command in wishes.0.drain(..) {
        g.feast.cards_command(me, command);
    }
    seen.view = g.feast.cards.as_ref().map(|table| table.view());
}

// ----- The tutorial (tutorial.rs): a guided first hand, offline.

mod tutorial;

/// Fully Loaded's tutorial: whether it is to be given (decided at this player's first table offline, from the address
/// and whether a browser has seen it: `tutorial::due`), the lesson under way, and a skip asked for this frame (`play`).
/// It is given once a run at the most.
#[derive(Resource, Default)]
pub struct Tutorial {
    due: Option<bool>,
    lesson: Option<tutorial::Lesson>,
    skip: bool,
}

/// Every frame, once the table is seen (`follow`): the tutorial. At this player's first table offline, when the lesson
/// is due (asked for: `tutorial::due`) and the game has only begun to be dealt, its table is laid in place of the game's (`Table::with_order`,
/// slowed by `tutorial::PACE`), with the same seats played by bots. While it is under way it takes in what happens at
/// the table; once the hand is over the table plays at a game's pace again, and when the last word has been up a while,
/// or the lesson is skipped, or this player is no longer at the table, it is over and remembered as seen (a skip also
/// puts the lesson's cards back into the deck). Online and in the demo it is never given.
fn teach(time: Res<Time<Real>>, options: Option<Res<crate::runtime::Options>>, mut g: ResMut<Game>, mut seen: ResMut<Seen>, mut tutorial: ResMut<Tutorial>) {
    let offline = at_table(&g) && !g.network && !g.demo;
    let me = seen.me.filter(|_| offline);
    let tutorial = &mut *tutorial;
    let Some(lesson) = tutorial.lesson.as_mut() else {
        tutorial.skip = false;
        let Some(me) = me else { return };
        let due = *tutorial.due.get_or_insert_with(|| tutorial::due(options.is_some_and(|o| o.tutorial()), tutorial::seen()));
        let Some(view) = g.feast.cards.as_ref().map(Table::view) else { return };
        // (A game only begun: no more than its first cards dealt.)
        let fresh = view.hand <= 1 && view.events.iter().all(|e| e.kind == Kind::Dealt);
        if !due || !fresh {
            return;
        }
        let seats = view.seats.len();
        let mut table = Table::with_order(seats, g.feast.card_seed, tutorial::dealer(seats, me), tutorial::order(seats, me));
        table.set_pace(tutorial::PACE);
        for (seat, s) in view.seats.iter().enumerate() {
            table.set_bot(seat, s.bot);
        }
        seen.view = Some(table.view());
        g.feast.cards = Some(table);
        (tutorial.lesson, tutorial.due) = (Some(tutorial::Lesson::default()), Some(false));
        return;
    };
    let skip = std::mem::take(&mut tutorial.skip);
    match (me, seen.view.as_ref()) {
        (Some(me), Some(view)) if !skip => lesson.hear(view, me, time.elapsed_secs()),
        _ => lesson.step = tutorial::Step::Done,
    }
    let step = lesson.step;
    if let Some(table) = g.feast.cards.as_mut() {
        match step {
            // (Over with the hand, the lesson's cards are back in the deck already; cut short, they go back now.)
            tutorial::Step::Done => {
                table.set_pace(1.);
                table.clear_order();
            }
            tutorial::Step::Wrap { .. } => table.set_pace(1.),
            _ => {}
        }
    }
    if step == tutorial::Step::Done {
        tutorial.lesson = None;
        tutorial::remember();
    }
}

// ----- The cards' faces (faces.rs): each kind of card printed once, with a picture of its food and its value in its
// corners.

mod faces;
pub use faces::card_face;
use faces::{MUSTARD, PAPER, image, print, print_back};

fn srgb(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}
fn rgb_of(c: Color) -> [f32; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue]
}
/// A colour between `c` and white.
fn lighter(c: [f32; 3], by: f32) -> [f32; 3] {
    c.map(|v| v + (1. - v) * by)
}

// ----- Where things are at the table.

/// Where the table stands: the middle of the microwave's top under it, the level way from there to the front seat
/// (`ahead`) and across it, and each seat's feet and the yaw that faces the middle (`feast::table_seats`, where the
/// simulation stands the hotdogs).
struct Spot {
    middle: Vec3,
    ahead: Vec3,
    across: Vec3,
    seats: Vec<(Vec3, f32)>,
}
impl Spot {
    fn new(level: &kitchen_sim::map::Level, seats: usize) -> Option<Spot> {
        let seats: Vec<(Vec3, f32)> = kitchen_sim::feast::table_seats(level, seats).into_iter().map(|(at, yaw)| (Vec3::from_array(at), yaw)).collect();
        let (first, yaw) = *seats.first()?;
        // The seats stand round the middle at equal angles, so theirs is the table's; a hotdog alone faces it.
        let mut middle = if seats.len() > 1 { seats.iter().map(|(at, _)| *at).sum::<Vec3>() / seats.len() as f32 } else { first + Vec3::new(-yaw.sin(), 0., -yaw.cos()) * RZ };
        middle.y = first.y - kitchen_sim::map::LIP as f32;
        let ahead = Vec3::new(first.x - middle.x, 0., first.z - middle.z).normalize_or(Vec3::NEG_Z);
        Some(Spot { middle, ahead, across: Vec3::Y.cross(ahead), seats })
    }
    /// The middle of the felt.
    fn felt(&self) -> Vec3 {
        self.middle + Vec3::Y * TOP
    }
    /// The level way from the table's middle to a seat.
    fn out(&self, seat: usize) -> Vec3 {
        let at = self.seats[seat.min(self.seats.len() - 1)].0;
        Vec3::new(at.x - self.middle.x, 0., at.z - self.middle.z).normalize_or(self.ahead)
    }
    /// Whether a point is on the felt, a margin of `inset` in from its edge.
    fn on(&self, at: Vec3, inset: f32) -> bool {
        let d = at - self.middle;
        let (x, z) = (d.dot(self.across) / (RX - inset), d.dot(self.ahead) / (RZ - inset));
        x * x + z * z <= 1.
    }
    /// How far the felt reaches from its middle the level way `way`.
    fn reach(&self, way: Vec3) -> f32 {
        let (x, z) = (way.dot(self.across) / RX, way.dot(self.ahead) / RZ);
        1. / (x * x + z * z).sqrt().max(1e-6)
    }
    /// This player's eye at seat `me`: BACK further from the table than its feet and EYE over the microwave's top.
    fn eye(&self, me: usize) -> Vec3 {
        let at = self.seats[me.min(self.seats.len() - 1)].0 + self.out(me) * BACK;
        Vec3::new(at.x, self.middle.y + EYE, at.z)
    }
}
/// The yaw that faces a hotdog at `from` towards `to` (a hotdog looks along (-sin yaw, -cos yaw)).
fn facing(from: Vec3, to: Vec3) -> f32 {
    f32::atan2(from.x - to.x, from.z - to.z)
}

/// Which of `seats` (each a point on a hotdog) is in sight from `eye` looking along `forward`: the one nearest to where
/// it looks, if that is within `max_angle` (radians) of it. A seat that is not a point (NaN: one that cannot be picked)
/// is never in sight.
pub fn seat_in_sight(eye: Vec3, forward: Vec3, seats: &[Vec3], max_angle: f32) -> Option<usize> {
    seats
        .iter()
        .enumerate()
        .filter(|(_, at)| at.is_finite() && at.distance(eye) > 1e-4)
        .map(|(i, at)| (i, forward.angle_between(*at - eye)))
        .filter(|(_, angle)| *angle <= max_angle)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// How the cards in front of a seat are set out, as an eye sees the table: in rows along the table's edge in front of
/// the hotdog (`along`, left to right as the eye sees a row across it, towards the eye one that runs away from it),
/// the first row nearest the hotdog and each next one a card nearer the middle, while the rows are clear of the draw
/// pile. Every card is turned upright to the eye (`upright`), so that each reads from this player's seat; so a card
/// takes up `wide` along its row and `deep` across it. Each row is the line its cards' middles may lie on (`rows`: its
/// middle, and how far they may go from it either way) for every card to lie on the felt and within the seat's share
/// of the table, so that no card lies in front of another hotdog.
struct Place {
    out: Vec3,
    along: Vec3,
    upright: Quat,
    wide: f32,
    deep: f32,
    rows: Vec<(Vec3, f32, f32)>,
}
/// Room between cards in a row, between rows and from the felt's edge, and the extra room a Napkin held lies apart by.
const ROOM: f32 = 0.002;
const APART: f32 = 0.004;
fn place(spot: &Spot, eye: Vec3, seat: usize) -> Place {
    let out = spot.out(seat);
    let reach = spot.reach(out);
    let towards = Vec3::new(spot.middle.x + out.x * reach * 0.7 - eye.x, 0., spot.middle.z + out.z * reach * 0.7 - eye.z).normalize_or(-spot.ahead);
    let right = towards.cross(Vec3::Y);
    let mut along = Vec3::Y.cross(out);
    let backwards = if along.dot(right).abs() >= along.dot(towards).abs() { along.dot(right) < 0. } else { along.dot(towards) > 0. };
    if backwards {
        along = -along;
    }
    let extent = |way: Vec3| way.dot(right).abs() * CARD.x + way.dot(towards).abs() * CARD.y;
    let (wide, deep) = (extent(along), extent(out));
    // Half the seat's share of the table, less a margin.
    let share = std::f32::consts::PI / spot.seats.len().max(2) as f32 - 0.06;
    let fits = |at: Vec3| {
        [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)].into_iter().all(|(a, o)| {
            let corner = at + along * (a * wide / 2.) + out * (o * deep / 2.);
            let from = Vec3::new(corner.x - spot.middle.x, 0., corner.z - spot.middle.z);
            spot.on(corner, 0.003) && from.angle_between(out) <= share
        })
    };
    // The first row as near the felt's edge as it fits.
    let mut first = reach - deep / 2. - 0.004;
    while first > 0.02 && !fits(spot.felt() + out * first) {
        first -= 0.001;
    }
    let mut rows = vec![];
    for k in 0..4 {
        let r = first - k as f32 * (deep + ROOM);
        // (The draw pile is about the middle: a card's corner reaches 0.0136 from it.)
        if k > 0 && r - deep / 2. < 0.018 {
            break;
        }
        let middle = spot.felt() + out * r;
        if !fits(middle) {
            break;
        }
        let (mut lo, mut hi) = (0., 0.);
        while lo > -0.2 && fits(middle + along * (lo - 0.001)) {
            lo -= 0.001;
        }
        while hi < 0.2 && fits(middle + along * (hi + 0.001)) {
            hi += 0.001;
        }
        rows.push((middle, lo, hi));
    }
    if rows.is_empty() {
        rows.push((spot.felt() + out * first, 0., 0.));
    }
    Place { out, along, upright: Quat::from_rotation_y(f32::atan2(-towards.x, -towards.z)), wide, deep, rows }
}
/// Where the cards in front of seat `seat` lie and how they are turned (`place`): in the order they came, filling the
/// first row and then the next; a row given more than it has room for closes up, a later card on top of an earlier
/// one. A Napkin held lies apart at the first row's start, turned a little.
fn spread(spot: &Spot, eye: Vec3, seat: usize, cards: &[Card]) -> Vec<(Card, Transform)> {
    let Place { out, along, upright, wide, rows, .. } = place(spot, eye, seat);
    let step = wide + ROOM;
    let napkin = cards.iter().position(|card| *card == Card::Napkin);
    let row: Vec<Card> = cards.iter().copied().filter(|card| *card != Card::Napkin).collect();
    // Each row's line (the first's after the Napkin), and how many cards it has room for.
    let lines: Vec<(Vec3, f32, f32)> = rows.iter().enumerate().map(|(k, (middle, lo, hi))| (*middle, if k == 0 && napkin.is_some() { (lo + step + APART).min(*hi) } else { *lo }, *hi)).collect();
    let room: Vec<usize> = lines.iter().map(|(_, lo, hi)| ((hi - lo) / step + 1e-4).floor() as usize + 1).collect();
    let mut counts = vec![0; lines.len()];
    let (mut left, total) = (row.len(), room.iter().sum::<usize>());
    for (k, count) in counts.iter_mut().enumerate() {
        // Room enough: each row as full as it goes. Too little: the cards shared out by the rows' lengths.
        let share = if row.len() <= total {
            room[k]
        } else {
            let length: f32 = lines.iter().map(|(_, lo, hi)| hi - lo + step).sum();
            ((row.len() as f32 * (lines[k].2 - lines[k].1 + step) / length).ceil() as usize).max(1)
        };
        *count = share.min(left);
        left -= *count;
    }
    if let Some(last) = counts.last_mut() {
        *last += left;
    }
    // (Rows never overlap: a card in a row that has closed up lies over the one before it.)
    let lift = |order: usize| Vec3::Y * THICK * (0.6 + 1.1 * order as f32);
    let mut laid = vec![];
    if let (Some(_), Some((middle, lo, _))) = (napkin, rows.first()) {
        laid.push((Card::Napkin, Transform::from_translation(*middle + along * *lo + out * 0.002 + lift(0)).with_rotation(upright * Quat::from_rotation_y(0.3))));
    }
    let mut next = row.iter();
    for ((middle, lo, hi), count) in lines.iter().zip(counts) {
        let (start, gap) = if count as f32 - 1. <= (hi - lo) / step + 1e-4 {
            ((lo + hi) / 2. - step * (count as f32 - 1.) / 2., step)
        } else {
            (*lo, (hi - lo) / (count as f32 - 1.).max(1.))
        };
        for i in 0..count {
            let Some(card) = next.next() else { break };
            laid.push((*card, Transform::from_translation(*middle + along * (start + gap * i as f32) + lift(i)).with_rotation(upright)));
        }
    }
    laid
}

// ----- The hotdogs.

/// How each hotdog at the table is to be posed (by its id), for the round's drawing of it to follow ([`seated`]):
/// worked out every frame (`pose`).
#[derive(Resource, Default)]
pub struct Poses {
    by: HashMap<u32, Posed>,
    /// When each seat last bust (real seconds), and the last of the table's events taken in (None: no table yet).
    busts: HashMap<usize, f32>,
    heard: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Posed {
    yaw: f32,
    cheer: bool,
    slump: bool,
}
impl Poses {
    /// Takes in the table's events since the last it heard: a bust among them slumps its hotdog from `now`. A table
    /// seen for the first time (a join, a new game: its events start again from 1) has its events taken as old news.
    fn hear(&mut self, view: &View, now: f32) {
        let last = view.events.iter().map(|e| e.seq).max().unwrap_or(0);
        match self.heard {
            Some(heard) if last >= heard => {
                for event in view.events.iter().filter(|e| e.seq > heard && e.kind == Kind::Bust) {
                    self.busts.insert(event.seat, now);
                }
            }
            _ => self.busts.clear(),
        }
        self.heard = Some(last);
    }
}
/// The yaw each seat's hotdog turns to: towards the seat whose go or pick it is; that hotdog itself, and every hotdog
/// while it is nobody's go, faces the table's middle.
fn heads(view: &View, seats: &[(Vec3, f32)]) -> Vec<f32> {
    let focus = view.choosing.map(|(seat, _)| seat).or(view.turn).filter(|seat| *seat < seats.len());
    seats
        .iter()
        .enumerate()
        .map(|(i, (at, yaw))| match focus {
            Some(seat) if seat != i => facing(*at, seats[seat].0),
            _ => *yaw,
        })
        .collect()
}
/// Whether a seat's hotdog cheers: it is fully loaded, or it has won.
fn cheers(view: &View, seat: usize) -> bool {
    view.winner == Some(seat) || view.seats.get(seat).is_some_and(|s| s.status == Status::Loaded)
}
/// Every frame at the table: how each hotdog is posed, its head turned smoothly to the seat whose go it is.
fn pose(time: Res<Time<Real>>, g: Res<Game>, seen: Res<Seen>, mut poses: ResMut<Poses>) {
    let (now, dt) = (time.elapsed_secs(), time.delta_secs().min(0.1));
    let view = seen.view.as_ref().filter(|_| at_table(&g));
    let Some((view, spot)) = view.and_then(|view| Some((view, Spot::new(&g.feast.level, view.seats.len())?))) else {
        if poses.heard.is_some() || !poses.by.is_empty() {
            *poses = Poses::default();
        }
        return;
    };
    poses.hear(view, now);
    let ease = 1. - (-dt * TURN_RATE).exp();
    for (seat, (p, wanted)) in g.roster.iter().zip(heads(view, &spot.seats)).enumerate() {
        let yaw = poses.by.get(&p.id).map_or(spot.seats[seat].1, |was| {
            let turn = (wanted - was.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            was.yaw + turn * ease
        });
        let slump = poses.busts.get(&seat).is_some_and(|at| now - at < SLUMP);
        poses.by.insert(p.id, Posed { yaw, cheer: cheers(view, seat), slump });
    }
}
/// At the card table (`table`), how hotdog `id` is drawn this frame rather than as the round has it (its yaw and its
/// cues): no gun in hand, turned towards whoever's go it is, cheering if it is fully loaded or has won (nobody else
/// cheers, the round's winner's cheer for everybody included), slumped for a moment after a bust. Away from the table
/// it is as it was. Called wherever the round poses its hotdogs: arena.rs in single player, network.rs online.
pub fn seated(world: &World, table: bool, id: u32, yaw: f32, cues: Cues) -> (f32, Cues) {
    if !table {
        return (yaw, cues);
    }
    dress(world.get_resource::<Poses>().and_then(|poses| poses.by.get(&id)).copied(), yaw, cues)
}
fn dress(posed: Option<Posed>, yaw: f32, cues: Cues) -> (f32, Cues) {
    let Posed { yaw, cheer, slump } = posed.unwrap_or(Posed { yaw, cheer: false, slump: false });
    (yaw, Cues { weapon: 0, cheer, crouch: slump, prone: false, stim: 0., carry: false, ..cues })
}

// ----- What is drawn: the table, the cards, the screen.

/// Everything the table puts in the world or on the screen (each thing at its root: what hangs from it goes with it).
#[derive(Component)]
struct Laid;
/// The cards on the table and the draw pile, dealt again whenever the table changes.
#[derive(Component)]
struct Dealt;
/// The scoreboard; one of its rows (the seat's); the prompt line; a name tag (the seat's).
#[derive(Component)]
struct Board;
#[derive(Component)]
struct BoardRow(usize);
#[derive(Component)]
struct Prompt;
#[derive(Component)]
struct Tag(usize);
/// The tutorial's coach box over the prompt line, and its skip note (a click on which skips the tutorial).
#[derive(Component)]
struct CoachBox;
#[derive(Component)]
struct SkipNote;

/// What is drawn of the table, and the free look from the seat.
#[derive(Resource, Default)]
struct Shown {
    /// The table is up (its furniture and the screen's panels): for so many seats, standing where `Spot::middle` and
    /// `Spot::ahead` say (a map reloaded by the editor with the microwave moved or turned has it laid again).
    up: Option<(usize, Vec3, Vec3)>,
    /// What the cards, the scoreboard and the name tags were dealt for (`Deal`), and when (real seconds) the table's
    /// version arrived: the clock it carries (online) counts down from then.
    dealt: Option<Deal>,
    arrived: f32,
    /// The free look: yaw and pitch (radians) from looking at the table's middle.
    look: Vec2,
    /// The hotdog in sight while this player picks (`play`), whose tag says so.
    sighted: Option<usize>,
    /// The pointer lock (`grip`): when (real seconds) the table last asked for it, and whether the page has had it since.
    lock_asked: f32,
    lock_seen: bool,
    /// The prompt as last laid out.
    line: Vec<Piece>,
    kit: Option<Kit>,
}
/// What the table is dealt for: its version and hand, and who sits where, which online can come after the table it goes
/// with (the names and colours come with the frames, and so does this player's seat): this player's seat and each
/// seat's name and colour. The table is dealt again when any of it changes.
#[derive(Clone, Debug, PartialEq)]
struct Deal {
    version: u64,
    hand: u32,
    me: Option<usize>,
    names: Vec<(String, [f32; 3])>,
}
/// What the cards are made of: a card, a block (the pile), each kind of card's material with its print, the back's, the
/// pile's edge's and the felt's, the wood's and the turn's mark. The kinds of card are printed PRINTS a frame from when
/// the kit is made, as the table is first laid (`Kit::print`), and the table is dealt once all of them are: no deal
/// waits on a print, nor any frame on all 22.
struct Kit {
    card: Handle<Mesh>,
    block: Handle<Mesh>,
    disc: Handle<Mesh>,
    faces: HashMap<Card, Handle<StandardMaterial>>,
    /// The kinds of card still to print, the next last.
    unprinted: Vec<Card>,
    back: Handle<StandardMaterial>,
    edge: Handle<StandardMaterial>,
    felt: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    mark: Handle<StandardMaterial>,
}
impl Kit {
    fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Kit {
        let mut kinds = kitchen_sim::cards::deck();
        kinds.dedup();
        kinds.reverse();
        Kit {
            card: meshes.add(Cuboid::new(CARD.x, THICK, CARD.y)),
            block: meshes.add(Cuboid::new(1., 1., 1.)),
            disc: meshes.add(Cylinder::new(1., 1.).mesh().resolution(96)),
            faces: HashMap::new(),
            unprinted: kinds,
            back: materials.add(card_material(images.add(image(print_back())))),
            edge: materials.add(StandardMaterial { base_color: srgb(PAPER), perceptual_roughness: 0.8, ..default() }),
            felt: materials.add(StandardMaterial { base_color: Color::srgb(0.09, 0.33, 0.21), perceptual_roughness: 0.97, reflectance: 0.2, ..default() }),
            wood: materials.add(StandardMaterial { base_color: Color::srgb(0.40, 0.24, 0.13), perceptual_roughness: 0.5, ..default() }),
            mark: materials.add(StandardMaterial { base_color: srgb(MUSTARD), emissive: LinearRgba::rgb(0.9, 0.6, 0.05), unlit: true, ..default() }),
        }
    }
    /// Prints up to PRINTS of the kinds of card still to print; and whether all of them are printed now.
    fn print(&mut self, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> bool {
        for _ in 0..PRINTS {
            let Some(card) = self.unprinted.pop() else { break };
            self.faces.insert(card, printed(card, materials, images));
        }
        self.unprinted.is_empty()
    }
    /// A card's material: printed with the kit, or (a card no deck has) the first time it is dealt.
    fn face(&mut self, card: Card, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Handle<StandardMaterial> {
        self.faces.entry(card).or_insert_with(|| printed(card, materials, images)).clone()
    }
}
/// How many kinds of card are printed a frame (`Kit::print`): the 22 take six frames, a couple of milliseconds each.
const PRINTS: usize = 4;
/// A card printed (`faces::print`), and its material.
fn printed(card: Card, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Handle<StandardMaterial> {
    materials.add(card_material(images.add(image(print(&card_face(card))))))
}
/// What a card is made of: card, with a print on it.
fn card_material(print: Handle<Image>) -> StandardMaterial {
    StandardMaterial { base_color_texture: Some(print), perceptual_roughness: 0.7, reflectance: 0.3, ..default() }
}

/// One row of the scoreboard: a seat's name in its colour, its cards this hand (by name, each in its colour), the
/// hand's points over its total, how it stands in the hand ("BUST", "STOPPED", "FRIDGED", "FULLY LOADED", "WINNER", or
/// nothing while it is still in), whether it is the seat whose go or pick it is (`up`), and whether it is this player's.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub colour: [f32; 3],
    pub cards: Vec<(String, [f32; 3])>,
    /// The cards themselves, which the scoreboard shows as chips (`chip`).
    pub hand: Vec<Card>,
    pub score: String,
    pub mark: &'static str,
    pub up: bool,
    pub me: bool,
}
/// The scoreboard's rows, one a seat in seat order, from the table and the seats' names and colours (`names`, in seat
/// order: a seat not named yet is called by its number).
pub fn rows(view: &View, names: &[(String, [f32; 3])], me: Option<usize>) -> Vec<Row> {
    let up = view.choosing.map(|(seat, _)| seat).or(view.turn);
    view.seats
        .iter()
        .enumerate()
        .map(|(i, seat)| {
            let (name, colour) = names.get(i).cloned().unwrap_or_else(|| (format!("Seat {}", i + 1), [0.8; 3]));
            let mark = match seat.status {
                _ if view.winner == Some(i) => "WINNER",
                Status::Bust => "BUST",
                Status::Stopped => "STOPPED",
                Status::Fridged => "FRIDGED",
                Status::Loaded => "FULLY LOADED",
                Status::In | Status::Out => "",
            };
            Row {
                name,
                colour,
                cards: seat.cards.iter().map(|card| (card.name(), rgb_of(card_face(*card).colour))).collect(),
                hand: seat.cards.clone(),
                score: format!("{} / {}", seat.points, seat.total),
                mark,
                up: up == Some(i),
                me: me == Some(i),
            }
        })
        .collect()
}
/// The seats' names and colours in seat order (the round's players, `roster`): the colour its player chose (`chosen`,
/// from the lobby: `Lobby::colour`), as its ring wears it, or without one (a bot, and offline) the colour of its place.
fn names(roster: &[Player], chosen: impl Fn(u32) -> Option<[f32; 3]>) -> Vec<(String, [f32; 3])> {
    roster.iter().map(|p| (p.name.clone(), chosen(p.id).unwrap_or(kitchen_sim::feast::COLORS[(p.id as usize).saturating_sub(1) % 5]))).collect()
}
/// Where a name tag `size` pixels big goes over a head at `spot` on the screen: its top-left corner, GAP over the head;
/// or, if it would lie over any of the scoreboard (`panel`), just below the scoreboard instead.
pub fn clear_of(spot: Vec2, size: Vec2, panel: Rect) -> Vec2 {
    let at = Vec2::new(spot.x - size.x / 2., spot.y - GAP - size.y);
    let over = Rect::from_corners(at, at + size).intersect(panel);
    if over.is_empty() { at } else { Vec2::new(at.x, panel.max.y + CLEAR) }
}
/// A piece of the prompt: words (in a colour, or the line's own), or an icon (in a colour, or white).
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text(String, Option<[f32; 3]>),
    Icon(Icon, Option<[f32; 3]>),
}
/// The prompt: what this player is to do now (on its go, the left button and a card, flip, the right and a hand, stop,
/// with the seconds `left` when there is a limit; picking, a target and the card to give), whose go it is otherwise,
/// each seat's points when a hand is over, and the winner. In pieces, words and icons (icons.rs), each with its colour.
/// With the touch controls showing (`touch`) the mouse buttons are left out: the screen's own buttons say it.
pub fn prompt(view: &View, names: &[(String, [f32; 3])], me: Option<usize>, left: Option<f32>, touch: bool) -> Vec<Piece> {
    let name = |seat: usize| names.get(seat).map_or(Piece::Text(format!("Seat {}", seat + 1), None), |(name, colour)| Piece::Text(name.clone(), Some(lighter(*colour, 0.3))));
    let words = |w: &str| Piece::Text(w.to_string(), None);
    let mouse = |button: Icon| (!touch).then_some(Piece::Icon(button, None));
    let clock = left.into_iter().flat_map(|left| [words("   "), Piece::Icon(Icon::Clock, None), words(&format!("{}", left.max(0.).ceil() as u32))]);
    if let Some(winner) = view.winner {
        let total = view.seats.get(winner).map_or(0, |s| s.total);
        return vec![Piece::Icon(Icon::Crown, Some(lighter(MUSTARD, 0.2))), name(winner), words(&format!(" {total}"))];
    }
    if view.between {
        let mut line = vec![];
        for (seat, s) in view.seats.iter().enumerate().filter(|(_, s)| s.status != Status::Out) {
            if !line.is_empty() {
                line.push(words("   "));
            }
            line.push(name(seat));
            line.push(if s.status == Status::Bust { Piece::Text(" BUST".into(), Some([1., 0.42, 0.36])) } else { words(&format!(" +{}", s.points)) });
        }
        return line;
    }
    match (view.choosing, view.turn) {
        (Some((seat, card)), _) if Some(seat) == me => {
            // (A hotdog looked at and clicked, or with the pointer freed (Esc) its row on the scoreboard; by touch, Pick, or Me.)
            [Some(Piece::Icon(Icon::Target, None)), Some(Piece::Text(card.name(), Some(lighter(rgb_of(card_face(card).colour), 0.3)))), mouse(Icon::MouseLeft)].into_iter().flatten().chain(clock).collect()
        }
        (None, Some(seat)) if Some(seat) == me => [mouse(Icon::MouseLeft), Some(Piece::Icon(Icon::Card, None)), Some(words("   ")), mouse(Icon::MouseRight), Some(Piece::Icon(Icon::Hand, None))].into_iter().flatten().chain(clock).collect(),
        (Some((seat, _)), _) | (None, Some(seat)) => vec![name(seat), words(" ...")],
        (None, None) => vec![Piece::Icon(Icon::Card, None), words(" ...")],
    }
}
/// A card as the scoreboard's chip shows it: its value ("12", "+2", "x2"), or for an action card and the Napkin an icon.
pub fn chip(card: Card) -> Result<String, Icon> {
    match card {
        Card::Topping(v) => Ok(format!("{v}")),
        Card::Side(v) => Ok(format!("+{v}")),
        Card::FootLong => Ok("x2".into()),
        Card::Fridge => Err(Icon::Fridge),
        Card::PileOn => Err(Icon::Pile),
        Card::Napkin => Err(Icon::Drape),
    }
}
/// How a seat stands, as the scoreboard shows it: an icon in a colour (none while it is still in).
fn standing(mark: &str) -> Option<(Icon, Color)> {
    let colour = mark_colour(mark);
    match mark {
        "WINNER" => Some((Icon::Crown, colour)),
        "BUST" => Some((Icon::Strike, colour)),
        "STOPPED" => Some((Icon::Hand, colour)),
        "FRIDGED" => Some((Icon::Fridge, colour)),
        "FULLY LOADED" => Some((Icon::Check, colour)),
        _ => None,
    }
}
/// The icons the table's screen shows, drawn for the window's pixels (`sf`, its scale factor).
struct IconKit<'a> {
    icons: &'a mut Icons,
    images: &'a mut Assets<Image>,
    sf: f32,
}
impl IconKit<'_> {
    /// An icon `px` logical pixels high, tinted.
    fn node(&mut self, icon: Icon, px: f32, tint: Color) -> (ImageNode, Node) {
        let height = (px * self.sf).round().max(8.) as u32;
        let image = self.icons.get(self.images, icon, height);
        let (w, h) = crate::icons::size(icon, height);
        (ImageNode { color: tint, ..ImageNode::new(image) }, Node { width: Val::Px(w as f32 / self.sf), height: Val::Px(h as f32 / self.sf), flex_shrink: 0., ..default() })
    }
}

/// Every frame: the table and what goes with it drawn while this player is at it (the cards printed a few a frame the
/// first time, and dealt once all are), the cards and the scoreboard dealt again when the table changes, the prompt line
/// written; and all of it gone when this player is not at the table.
#[allow(clippy::too_many_arguments)]
fn lay(
    mut commands: Commands,
    time: Res<Time<Real>>,
    g: Res<Game>,
    lobby: Option<Res<crate::lobby::Lobby>>,
    seen: Res<Seen>,
    mut shown: ResMut<Shown>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    laid: Query<Entity, With<Laid>>,
    dealt: Query<Entity, With<Dealt>>,
    panels: Query<Entity, With<Board>>,
    prompts: Query<Entity, With<Prompt>>,
    mut tags: Query<(Entity, &Tag, &mut Text, &mut TextColor)>,
    (mut icons, windows): (ResMut<Icons>, Query<&Window>),
) {
    let table = seen.view.as_ref().filter(|_| at_table(&g)).and_then(|view| Some((view, Spot::new(&g.feast.level, view.seats.len())?)));
    let Some((view, spot)) = table else {
        if shown.up.is_some() {
            for e in &laid {
                commands.entity(e).despawn();
            }
            (shown.up, shown.dealt, shown.sighted, shown.look) = (None, None, None, Vec2::ZERO);
            shown.line.clear();
        }
        return;
    };
    let shown = &mut *shown;
    let kit = shown.kit.get_or_insert_with(|| Kit::new(&mut meshes, &mut materials, &mut images));
    let printed = kit.print(&mut materials, &mut images);
    let names = names(&g.roster, |id| lobby.as_ref().and_then(|lobby| lobby.colour(id)));
    let standing = (view.seats.len(), spot.middle, spot.ahead);
    if shown.up != Some(standing) {
        for e in &laid {
            commands.entity(e).despawn();
        }
        furnish(&mut commands, kit, &spot);
        screen(&mut commands, view.seats.len());
        (shown.up, shown.dealt) = (Some(standing), None);
        shown.line.clear();
        // (What is spawned now is filled in next frame.)
        return;
    }
    if !printed {
        // (The cards, the scoreboard and the prompt come with the first deal, a few frames on.)
        return;
    }
    let now = time.elapsed_secs();
    let fresh = shown.dealt.as_ref().is_none_or(|d| d.version != view.version || d.hand != view.hand);
    if fresh || shown.dealt.as_ref().is_some_and(|d| d.me != seen.me || d.names != names) {
        // (The clock counts from when the table changed, not from a name.)
        if fresh {
            shown.arrived = now;
        }
        shown.dealt = Some(Deal { version: view.version, hand: view.hand, me: seen.me, names: names.clone() });
        for e in &dealt {
            commands.entity(e).despawn();
        }
        deal(&mut commands, kit, &mut materials, &mut images, &spot, view, seen.me);
        if let Ok(panel) = panels.single() {
            let sf = windows.iter().next().map_or(1., |w| w.scale_factor());
            board(&mut commands, &mut IconKit { icons: &mut icons, images: &mut images, sf }, panel, view, &names, seen.me);
        }
        let marks: Vec<&str> = rows(view, &names, seen.me).iter().map(|row| row.mark).collect();
        for (e, tag, mut text, mut colour) in &mut tags {
            let (name, rgb) = names.get(tag.0).cloned().unwrap_or_else(|| (format!("Seat {}", tag.0 + 1), [0.8; 3]));
            text.0 = plain(&name);
            colour.0 = srgb(lighter(rgb, 0.3));
            commands.entity(e).despawn_children();
            let mark = marks.get(tag.0).copied().unwrap_or("");
            if !mark.is_empty() {
                commands.entity(e).with_child((TextSpan::new(format!("  {mark}")), TextFont { font_size: FontSize::Px(11.), ..default() }, TextColor(mark_colour(mark))));
            }
        }
    }
    let left = view.clock.map(|clock| clock - (now - shown.arrived));
    let line = prompt(view, &names, seen.me, left, crate::touch::on());
    if line != shown.line {
        if let Ok(e) = prompts.single() {
            let sf = windows.iter().next().map_or(1., |w| w.scale_factor());
            let mut kit = IconKit { icons: &mut icons, images: &mut images, sf };
            let shadow = TextShadow { offset: Vec2::splat(1.), color: Color::srgba(0., 0., 0., 0.6) };
            commands.entity(e).despawn_children().with_children(|row| {
                for piece in &line {
                    match piece {
                        Piece::Text(words, colour) => {
                            row.spawn((Text::new(plain(words)), TextFont { font_size: FontSize::Px(16.), ..default() }, TextColor(colour.map_or(Color::WHITE, srgb)), TextLayout::no_wrap(), shadow));
                        }
                        Piece::Icon(icon, colour) => {
                            row.spawn(kit.node(*icon, 24., colour.map_or(Color::WHITE, srgb)));
                        }
                    }
                }
            });
        }
        shown.line = line;
    }
}
/// The table: a foot, a leg, a wooden rim and the felt on it.
fn furnish(commands: &mut Commands, kit: &Kit, spot: &Spot) {
    let turn = Quat::from_rotation_y(f32::atan2(spot.ahead.x, spot.ahead.z));
    let mut part = |material: &Handle<StandardMaterial>, at: f32, size: Vec3| {
        commands.spawn((Laid, Mesh3d(kit.disc.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(spot.middle + Vec3::Y * at).with_rotation(turn).with_scale(size)));
    };
    part(&kit.wood, 0.002, Vec3::new(0.034, 0.004, 0.034));
    part(&kit.wood, (TOP - 0.01) / 2. + 0.002, Vec3::new(0.007, TOP - 0.014, 0.007));
    part(&kit.wood, TOP - 0.0065, Vec3::new(RX + 0.004, 0.009, RZ + 0.004));
    part(&kit.felt, TOP - 0.001, Vec3::new(RX, 0.002, RZ));
}
/// The cards on the table, as `view` has them: each seat's in front of it, upright to this player's eye; the draw pile
/// in the middle as high as the deck; and a mark of mustard along the front of the row of the seat whose go it is.
fn deal(commands: &mut Commands, kit: &mut Kit, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, spot: &Spot, view: &View, me: Option<usize>) {
    let eye = spot.eye(me.unwrap_or(0));
    for (seat, s) in view.seats.iter().enumerate().take(spot.seats.len()) {
        for (card, at) in spread(spot, eye, seat, &s.cards) {
            let material = kit.face(card, materials, images);
            commands.spawn((Laid, Dealt, Mesh3d(kit.card.clone()), MeshMaterial3d(material), at, NotShadowCaster));
        }
    }
    let towards = Vec3::new(spot.middle.x - eye.x, 0., spot.middle.z - eye.z).normalize_or(-spot.ahead);
    let upright = Quat::from_rotation_y(f32::atan2(-towards.x, -towards.z) + 0.12);
    if view.deck > 0 {
        let high = (view.deck as f32 * PILE_THICK).max(THICK);
        let pile = spot.felt() + Vec3::Y * high / 2.;
        commands.spawn((Laid, Dealt, Mesh3d(kit.block.clone()), MeshMaterial3d(kit.edge.clone()), Transform::from_translation(pile).with_rotation(upright).with_scale(Vec3::new(CARD.x * 0.98, high, CARD.y * 0.98))));
        commands.spawn((Laid, Dealt, Mesh3d(kit.card.clone()), MeshMaterial3d(kit.back.clone()), Transform::from_translation(spot.felt() + Vec3::Y * (high + THICK / 2.)).with_rotation(upright), NotShadowCaster));
    }
    if let Some(seat) = view.choosing.map(|(seat, _)| seat).or(view.turn).filter(|seat| *seat < spot.seats.len()) {
        let Place { out, along, deep, rows, .. } = place(spot, eye, seat);
        let (middle, lo, hi) = rows[0];
        let at = middle + along * ((lo + hi) / 2.) + out * (deep / 2. + 0.003) + Vec3::Y * 0.0003;
        let turn = Quat::from_rotation_y(f32::atan2(along.x, along.z));
        let length = (hi - lo + CARD.x).max(CARD.x * 2.);
        commands.spawn((Laid, Dealt, Mesh3d(kit.block.clone()), MeshMaterial3d(kit.mark.clone()), Transform::from_translation(at).with_rotation(turn).with_scale(Vec3::new(0.0018, 0.0004, length)), NotShadowCaster));
    }
}
/// The screen's parts, empty until the table is dealt: the scoreboard top-left, the prompt line along the bottom, the
/// tutorial's coach box over it (laid out only during the tutorial), and a name tag for each seat. (This player's own cards
/// are those on the table in front of its seat, not drawn again on the screen.)
fn screen(commands: &mut Commands, seats: usize) {
    commands.spawn((
        Laid,
        Board,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(14.),
            top: Val::Px(14.),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.),
            padding: UiRect::all(Val::Px(8.)),
            border_radius: BorderRadius::all(Val::Px(8.)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.8)),
        ZIndex(2),
    ));
    // Along the bottom, the prompt line, and over it (in the same column, so that the two never overlap however many
    // lines either takes) the tutorial's coach box, laid out only during the tutorial.
    commands
        .spawn((
            Laid,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                right: Val::Px(0.),
                bottom: Val::Px(14.),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(8.),
                ..default()
            },
            ZIndex(1),
        ))
        .with_children(|bar| {
            bar.spawn((
                CoachBox,
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(4.),
                    padding: UiRect::new(Val::Px(14.), Val::Px(14.), Val::Px(8.), Val::Px(7.)),
                    border: UiRect::all(Val::Px(1.)),
                    border_radius: BorderRadius::all(Val::Px(8.)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.84)),
                BorderColor::all(srgb(MUSTARD)),
            ));
            bar.spawn((
                Prompt,
                Node { padding: UiRect::axes(Val::Px(12.), Val::Px(5.)), border_radius: BorderRadius::all(Val::Px(6.)), align_items: AlignItems::Center, column_gap: Val::Px(5.), ..default() },
                BackgroundColor(Color::srgba(0., 0., 0., 0.5)),
            ));
        });
    for seat in 0..seats {
        commands.spawn((
            Laid,
            Tag(seat),
            Text::default(),
            TextFont { font_size: FontSize::Px(13.), ..default() },
            TextColor(Color::WHITE),
            TextLayout::no_wrap(),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::new(Val::Px(6.), Val::Px(6.), Val::Px(1.), Val::Px(2.)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(4.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.8)),
            BorderColor::all(Color::NONE),
            Visibility::Hidden,
        ));
    }
}
fn mark_colour(mark: &str) -> Color {
    match mark {
        "BUST" => Color::srgb(1., 0.42, 0.36),
        "STOPPED" => Color::srgb(0.72, 0.8, 0.9),
        "FRIDGED" => Color::srgb(0.62, 0.88, 1.),
        _ => Color::srgb(1., 0.86, 0.3),
    }
}
/// Text as the screen's font prints it: the font has only ASCII, so Jalapenos, Foot-Long x2 and "thinking...".
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ñ' => "n".into(),
            'Ñ' => "N".into(),
            '×' => "x".into(),
            '…' => "...".into(),
            c if c.is_ascii() => c.to_string(),
            _ => "?".into(),
        })
        .collect()
}
/// The scoreboard's rows (`rows`), under a line with the hand's number and the points to win, each by an icon. A row a
/// seat: its name in its colour, the hand's points over the total and how it stands, as an icon; under them its cards
/// as chips in their colours, each with its value (or an action card's icon: `chip`). The row of the seat whose go it
/// is is highlighted, and this player's is a shade lighter.
fn board(commands: &mut Commands, kit: &mut IconKit, panel: Entity, view: &View, names: &[(String, [f32; 3])], me: Option<usize>) {
    let font = |px: f32| TextFont { font_size: FontSize::Px(px), ..default() };
    let dim = Color::srgb(0.62, 0.7, 0.66);
    let (hand, target) = (kit.node(Icon::Card, 16., dim), kit.node(Icon::Crown, 16., dim));
    let rows: Vec<Row> = rows(view, names, me);
    let marks: Vec<Option<_>> = rows.iter().map(|row| standing(row.mark).map(|(icon, colour)| kit.node(icon, 15., colour))).collect();
    let chips: Vec<Vec<(Result<String, _>, [f32; 3])>> = rows
        .iter()
        .map(|row| row.hand.iter().zip(&row.cards).map(|(card, (_, colour))| (chip(*card).map_err(|icon| kit.node(icon, 11., Color::WHITE)), *colour)).collect())
        .collect();
    commands.entity(panel).despawn_children().with_children(|panel| {
        panel
            .spawn(Node { column_gap: Val::Px(4.), align_items: AlignItems::Center, margin: UiRect::new(Val::Px(6.), Val::Px(0.), Val::Px(0.), Val::Px(3.)), ..default() })
            .with_children(|top| {
                top.spawn(hand);
                top.spawn((Text::new(format!("{}", view.hand.max(1))), font(11.), TextColor(dim), TextLayout::no_wrap()));
                top.spawn(Node { width: Val::Px(6.), ..default() });
                top.spawn(target);
                top.spawn((Text::new(format!("{}", kitchen_sim::cards::TARGET)), font(11.), TextColor(dim), TextLayout::no_wrap()));
            });
        for (seat, ((row, mark), chips)) in rows.into_iter().zip(marks).zip(chips).enumerate() {
            let tint = if row.up {
                Color::srgba(1., 0.82, 0.25, 0.24)
            } else if row.me {
                Color::srgba(1., 1., 1., 0.07)
            } else {
                Color::NONE
            };
            panel
                .spawn((
                    BoardRow(seat),
                    Node {
                        width: Val::Px(236.),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.),
                        padding: UiRect::new(Val::Px(6.), Val::Px(6.), Val::Px(2.), Val::Px(4.)),
                        border: UiRect::left(Val::Px(3.)),
                        border_radius: BorderRadius::all(Val::Px(4.)),
                        ..default()
                    },
                    BackgroundColor(tint),
                    BorderColor::all(if row.up { srgb(MUSTARD) } else { Color::NONE }),
                ))
                .with_children(|lines| {
                    lines.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|line| {
                        line.spawn((Text::new(plain(&row.name)), font(12.), TextColor(srgb(lighter(row.colour, 0.3))), TextLayout::no_wrap()));
                        line.spawn(Node { column_gap: Val::Px(5.), align_items: AlignItems::Center, ..default() }).with_children(|right| {
                            if let Some(mark) = mark {
                                right.spawn(mark);
                            }
                            right.spawn((Text::new(row.score.clone()), font(12.), TextColor(Color::srgb(0.88, 0.9, 0.88)), TextLayout::no_wrap()));
                        });
                    });
                    if chips.is_empty() {
                        return;
                    }
                    lines.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(3.), row_gap: Val::Px(3.), ..default() }).with_children(|cards| {
                        for (face, colour) in chips {
                            let mut chip = cards.spawn((
                                Node { min_width: Val::Px(18.), height: Val::Px(15.), padding: UiRect::axes(Val::Px(3.), Val::Px(0.)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(3.)), ..default() },
                                BackgroundColor(srgb(colour.map(|c| c * 0.82))),
                            ));
                            match face {
                                Ok(value) => {
                                    chip.with_child((Text::new(value), font(10.), TextColor(Color::WHITE), TextLayout::no_wrap(), TextShadow { offset: Vec2::splat(1.), color: Color::srgba(0., 0., 0., 0.7) }));
                                }
                                Err(icon) => {
                                    chip.with_child(icon);
                                }
                            }
                        }
                    });
                });
        }
    });
}
/// How many characters a line of what the tutorial's coach box says takes at the most before it goes on to the next
/// (the font's characters are all one width, CHAR pixels at its size: a line is about 560 pixels), and at the least on
/// a narrow window (a phone held upright), where a line is as wide as the window leaves room for.
const COACH_LINE: usize = 62;
const COACH_SHORT: usize = 24;
const CHAR: f32 = 9.;
/// The characters a line of the coach box takes in a window `width` pixels across (logical): as many as fit with room
/// for the box's padding and a margin, between COACH_SHORT and COACH_LINE.
fn coach_line(width: f32) -> usize {
    (((width - 60.) / CHAR).floor().max(0.) as usize).clamp(COACH_SHORT, COACH_LINE)
}
/// Text broken into lines of at most `width` characters at its spaces (a longer word is a line of its own). As on the
/// scoreboard, the lines are broken here, never by the layout.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = 0;
    for word in text.split(' ').filter(|word| !word.is_empty()) {
        if line > 0 && line + 1 + word.len() > width {
            out.push('\n');
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out
}
/// Every frame, once the table is laid out (`lay`): the tutorial's coach box says what the lesson's step says
/// (`tutorial::coach`): the lesson, what to do now in mustard, and the skip note, small, its lines as long as the
/// window has room for (`coach_line`). It is laid out only while a lesson is under way at a table that has been dealt,
/// and written again only when what it says, or the room for it, changes.
#[allow(clippy::type_complexity)]
fn coach_box(
    mut commands: Commands,
    g: Res<Game>,
    seen: Res<Seen>,
    tutorial: Res<Tutorial>,
    shown: Res<Shown>,
    windows: Query<&Window>,
    mut boxes: Query<(Entity, &mut Node), With<CoachBox>>,
    mut said: Local<Option<(Entity, usize, Option<tutorial::Coach>)>>,
) {
    let Ok((e, mut node)) = boxes.single_mut() else {
        *said = None;
        return;
    };
    let lesson = tutorial.lesson.as_ref().filter(|_| at_table(&g) && shown.dealt.is_some());
    let coach = lesson.zip(seen.view.as_ref()).zip(seen.me).and_then(|((lesson, view), me)| tutorial::coach(lesson.step, view, me, crate::touch::on()));
    let line = windows.iter().next().map_or(COACH_LINE, |window| coach_line(window.width()));
    if said.as_ref().is_some_and(|(was, room, before)| *was == e && *room == line && *before == coach) {
        return;
    }
    let display = if coach.is_some() { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
    let font = |px: f32| TextFont { font_size: FontSize::Px(px), ..default() };
    commands.entity(e).despawn_children();
    if let Some(coach) = &coach {
        commands.entity(e).with_children(|lines| {
            let centred = TextLayout::no_wrap().with_justify(Justify::Center);
            lines.spawn((Text::new(plain(&wrap(&coach.say, line))), font(15.), TextColor(Color::WHITE), centred));
            if let Some(act) = &coach.act {
                lines.spawn((Text::new(plain(&wrap(act, line))), font(15.), TextColor(srgb(lighter(MUSTARD, 0.25))), centred));
            }
            if let Some(skip) = coach.skip {
                lines.spawn((SkipNote, Text::new(skip), font(11.), TextColor(Color::srgb(0.62, 0.66, 0.66)), centred, Node { margin: UiRect::top(Val::Px(2.)), ..default() }));
            }
        });
    }
    *said = Some((e, line, coach));
}

// ----- The seat: the camera, the input, the tags.

/// Every frame at the table: this player's eye at its seat, looking at the table's middle and about it as the mouse
/// moves while the pointer is captured.
fn sit(
    g: Res<Game>,
    seen: Res<Seen>,
    motion: Res<AccumulatedMouseMotion>,
    settings: Option<Res<crate::settings::Settings>>,
    pad: Option<Res<crate::touch::Pad>>,
    cursors: Query<&CursorOptions>,
    mut shown: ResMut<Shown>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<HotdogCamera>>,
) {
    let Some(spot) = seen.view.as_ref().filter(|_| at_table(&g)).and_then(|view| Spot::new(&g.feast.level, view.seats.len())) else {
        return;
    };
    // (The mouse while it is captured, and a drag on the touch controls.)
    let captured = cursors.iter().any(|cursor| cursor.grab_mode == CursorGrabMode::Locked);
    let drag = pad.filter(|pad| pad.on).map_or(Vec2::ZERO, |pad| pad.look);
    // (The mouse at the player's sensitivity and Y, settings.rs.)
    let look = settings.map_or(Vec2::ONE, |s| s.look());
    let moved = if captured { motion.delta * look } else { Vec2::ZERO } + drag;
    if moved != Vec2::ZERO {
        let (most_x, most_y) = (LOOK.x.to_radians(), LOOK.y.to_radians());
        let look = shown.look - moved * SENSITIVITY;
        shown.look = Vec2::new(look.x.clamp(-most_x, most_x), look.y.clamp(-most_y, most_y));
    }
    let eye = spot.eye(seen.me.unwrap_or(0));
    let to = spot.felt() + Vec3::Y * AIM - eye;
    let yaw = f32::atan2(-to.x, -to.z) + shown.look.x;
    let pitch = (to.y.atan2(Vec2::new(to.x, to.z).length()) + shown.look.y).clamp(-1.48, 1.48);
    let view = Transform::from_translation(eye).with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch));
    for (mut transform, mut projection) in &mut cameras {
        *transform = view;
        if let Projection::Perspective(lens) = &mut *projection {
            (lens.fov, lens.near) = (FOV.to_radians(), 0.004);
        }
    }
}
/// A node's place on the screen, in the window's (logical) pixels.
fn rect(laid: &ComputedNode, at: &UiGlobalTransform) -> Rect {
    Rect::from_center_half_size(at.translation * laid.inverse_scale_factor, laid.size() * laid.inverse_scale_factor / 2.)
}
/// What a left click off the scoreboard does at the table: whether it captures the pointer, and what it asks of the
/// table. With the pointer free it only captures it: it is the click that comes back to the game after Esc, the chat
/// box, another window or a lock the browser let go, and a flip could ruin the hand and a pick cannot be taken back.
/// Captured, it flips on this player's go, and picks the hotdog in sight (`sighted`) while it picks.
fn click(captured: bool, go: bool, picking: bool, sighted: Option<usize>) -> (bool, Option<Command>) {
    if !captured {
        return (true, None);
    }
    (false, if go { Some(Command::Flip) } else if picking { sighted.map(Command::Target) } else { None })
}
/// What the right button asks of the table: on this player's go it stops. While it picks who gets an action card it
/// asks nothing with the mouse (a click on a scoreboard row, or on the hotdog in sight, picks), but by touch (`touch`:
/// the controls are showing, whose Stop then reads "Me", touch.js) it picks this player's own hotdog, `me`: the one way
/// to give the card to oneself with no pointer to click the scoreboard's row and no hotdog to look at in one's own seat.
fn scope(go: bool, picking: bool, touch: bool, me: usize) -> Option<Command> {
    if go {
        Some(Command::Stop)
    } else {
        (picking && touch).then_some(Command::Target(me))
    }
}
/// The pointer as the table finds it, from the window's grab (`locked`), whether the page really has the pointer lock
/// (`really`: in a browser the document's; natively the grab is the truth, so always), whether it has had it since the
/// grab was asked for (`seen`), and how long ago that was (`waited`, seconds). Returns whether it is captured, whether
/// the grab is to be let go, and `seen` now.
///
/// A browser can let the lock go without the game hearing Esc (Esc itself, another tab or window): the grab still says
/// locked, but the pointer is not captured, and the grab is let go so that the next click asks for the lock again (and
/// only does that). A lock asked for comes a moment later, and is not lost meanwhile, unless it has not come within
/// PATIENCE (the browser refused it).
fn grip(locked: bool, really: bool, seen: bool, waited: f32) -> (bool, bool, bool) {
    match (locked, really) {
        (true, true) => (true, false, true),
        (true, false) if seen || waited > PATIENCE => (false, true, false),
        _ => (false, false, false),
    }
}
/// Whether the page has the pointer lock (`grip`): in a browser, whether the document has a pointer lock element;
/// natively the window's grab is the truth, so always.
fn page_locked() -> bool {
    #[cfg(target_arch = "wasm32")]
    return cards_locked();
    #[cfg(not(target_arch = "wasm32"))]
    true
}
/// Every frame, before the table is played (`follow`): what this player's buttons ask of it. On its go the left button
/// flips and the right stops; picking, the left picks the hotdog the eye looks at (`seat_in_sight`) while the pointer is
/// captured, and with it free (Esc) the one whose scoreboard row is clicked. A click anywhere but on the scoreboard with
/// the pointer free only captures it, as in the round (Esc frees it, arena.rs): it neither flips nor picks (`click`).
/// In a browser the pointer counts as captured only while the page really has the lock (`grip`), so the click after
/// a lock the browser let go unheard only captures it too. One on the scoreboard does nothing else.
///
/// The touch controls (touch.rs) play the same way with no pointer: their trigger is the left button, picking the
/// hotdog the eye looks at, and their scope the right, which stops at the go and, while it picks, gives the card to
/// this player itself (`scope`), as the eye never looks at its own seat.
///
/// During the tutorial K skips it, and so does a click on its note with the pointer free (the click captures nothing);
/// by touch, while the lesson asks for a flip, Stop skips it rather than stopping (its note says so).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn play(
    time: Res<Time<Real>>,
    g: Res<Game>,
    seen: Res<Seen>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Option<Res<crate::controls::Bindings>>,
    pad: Option<Res<crate::touch::Pad>>,
    mut windows: Query<(&Window, &mut CursorOptions)>,
    cameras: Query<(&Camera, &GlobalTransform), With<HotdogCamera>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<Board>>,
    rows: Query<(&BoardRow, &ComputedNode, &UiGlobalTransform)>,
    notes: Query<(&ComputedNode, &UiGlobalTransform), With<SkipNote>>,
    mut shown: ResMut<Shown>,
    mut wishes: ResMut<Wishes>,
    mut tutorial: ResMut<Tutorial>,
) {
    shown.sighted = None;
    let (Some(view), Some(me)) = (seen.view.as_ref().filter(|_| at_table(&g)), seen.me) else {
        return;
    };
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    // The pointer lock as the page has it (a browser can let it go unheard): let go of the grab when the lock has gone.
    let now = time.elapsed_secs();
    let (captured, release, had) = grip(cursor.grab_mode == CursorGrabMode::Locked, page_locked(), shown.lock_seen, now - shown.lock_asked);
    shown.lock_seen = had;
    if release {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
    if view.winner.is_some() {
        return;
    }
    // (While the page's chat box, or anything else, has the keyboard the mouse is not heard. The touch controls'
    // buttons are, as the round hears them: on a phone the game may never have the keyboard back once the page's
    // boxes have had it, and a tap on Flip is meant.)
    let heard = window.focused;
    // (With the touch controls showing, the eye is where a drag has turned it and the pointer has no say.)
    let touch = pad.map(|pad| *pad).filter(|pad| pad.on).unwrap_or_default();
    let pointer = window.cursor_position().filter(|_| !captured && !touch.on);
    let on_board = pointer.is_some_and(|at| panels.iter().any(|(laid, place)| rect(laid, place).contains(at)));
    let lesson = tutorial.lesson.clone();
    let on_note = lesson.is_some() && pointer.is_some_and(|at| notes.iter().any(|(laid, place)| rect(laid, place).contains(at)));
    if heard && lesson.is_some() && keys.just_pressed(KeyCode::KeyK) {
        tutorial.skip = true;
    }
    let (go, picking) = asks(view, Some(me));
    if picking && !on_board {
        let spot = Spot::new(&g.feast.level, view.seats.len());
        let chests: Vec<Vec3> = view
            .seats
            .iter()
            .enumerate()
            .map(|(seat, s)| match &spot {
                Some(spot) if seat != me && s.status == Status::In && seat < spot.seats.len() => spot.seats[seat].0 + Vec3::Y * CHEST,
                _ => Vec3::NAN,
            })
            .collect();
        // (Where the eye looks: a hotdog is picked only with the pointer captured, or by touch.)
        let ray = cameras.iter().find(|(camera, _)| camera.is_active).and_then(|(_, place)| (captured || touch.on).then(|| (place.translation(), *place.forward())));
        shown.sighted = ray.and_then(|(eye, ahead)| seat_in_sight(eye, ahead, &chests, SIGHT.to_radians()));
    }
    // The trigger and the aim as the player has them bound (controls.rs: the left and right buttons to begin with). The
    // left button with the pointer free is the pointer's (the scoreboard, the note, capturing), whatever the trigger is.
    let bindings = bindings.map_or_else(crate::controls::Bindings::default, |b| b.clone());
    let fire = bindings.just(crate::controls::Action::Fire, &keys, &mouse);
    let stop = bindings.just(crate::controls::Action::Aim, &keys, &mouse);
    if heard && captured && fire {
        let (_, wish) = click(true, go, picking, shown.sighted);
        wishes.0.extend(wish);
    } else if heard && !captured && mouse.just_pressed(MouseButton::Left) {
        if on_board {
            let row = pointer.and_then(|at| rows.iter().find(|(_, laid, place)| rect(laid, place).contains(at)).map(|(row, _, _)| row.0));
            if let Some(seat) = row.filter(|seat| picking && view.seats.get(*seat).is_some_and(|s| s.status == Status::In)) {
                wishes.0.push(Command::Target(seat));
            }
        } else if on_note {
            tutorial.skip = true;
        } else {
            let (capture, wish) = click(captured, go, picking, shown.sighted);
            if capture {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
                (shown.lock_asked, shown.lock_seen) = (now, false);
            }
            wishes.0.extend(wish);
        }
    }
    if touch.pulled {
        if go {
            wishes.0.push(Command::Flip);
        } else if let Some(seat) = shown.sighted.filter(|_| picking) {
            wishes.0.push(Command::Target(seat));
        }
    }
    // (The touch controls' Stop is the right button too, touch.rs, and "Me" while it picks: `scope`.)
    if stop && !on_board && (heard || touch.on) {
        if touch.on && lesson.as_ref().is_some_and(tutorial::Lesson::stop_skips) {
            tutorial.skip = true;
        } else {
            wishes.0.extend(scope(go, picking, touch.on, me));
        }
    }
}
/// Every frame, once the camera and the hotdogs have their places: each other hotdog's name tag over its head, kept
/// on the screen and clear of the scoreboard; its edge mustard while it is its go, white while this player has it in
/// sight to pick.
#[allow(clippy::type_complexity)]
fn label(
    g: Option<Res<Game>>,
    lobby: Option<Res<crate::lobby::Lobby>>,
    seen: Res<Seen>,
    shown: Res<Shown>,
    cameras: Query<(&Camera, &GlobalTransform), With<HotdogCamera>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<Board>>,
    mut tags: Query<(&Tag, &mut Node, &ComputedNode, &mut Visibility, &mut BorderColor)>,
) {
    if tags.is_empty() {
        return;
    }
    let camera = cameras.iter().find(|(camera, _)| camera.is_active);
    let spot = g.as_ref().zip(seen.view.as_ref()).and_then(|(g, view)| Spot::new(&g.feast.level, view.seats.len()));
    let panel = panels.iter().next().map(|(laid, at)| rect(laid, at));
    let up = seen.view.as_ref().and_then(|view| view.choosing.map(|(seat, _)| seat).or(view.turn));
    let names = g.as_ref().map(|g| names(&g.roster, |id| lobby.as_ref().and_then(|lobby| lobby.colour(id)))).unwrap_or_default();
    for (tag, mut node, laid, mut shown_tag, mut edge) in &mut tags {
        let size = laid.size() * laid.inverse_scale_factor;
        let place = camera.zip(spot.as_ref()).filter(|_| Some(tag.0) != seen.me && size.x > 0.).and_then(|((camera, view), spot)| {
            let head = spot.seats.get(tag.0)?.0 + Vec3::Y * HEAD;
            let screen = camera.logical_viewport_size()?;
            let at = camera.world_to_viewport(view, head).ok().filter(|_| (head - view.translation()).dot(*view.forward()) > 0.)?;
            // Kept on the screen (a head over the top of it has its tag along the top), then clear of the scoreboard.
            let at = Vec2::new(at.x.clamp(size.x / 2. + 4., (screen.x - size.x / 2. - 4.).max(size.x / 2. + 4.)), at.y.max(size.y + GAP + 4.));
            Some(panel.map_or(Vec2::new(at.x - size.x / 2., at.y - GAP - size.y), |panel| clear_of(at, size, panel)))
        });
        let want = if place.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *shown_tag != want {
            *shown_tag = want;
        }
        let Some(at) = place else { continue };
        let (left, top) = (Val::Px(at.x.round()), Val::Px(at.y.round()));
        if node.left != left || node.top != top {
            (node.left, node.top) = (left, top);
        }
        let own = names.get(tag.0).map_or([0.8; 3], |(_, rgb)| *rgb);
        let (rim, width) = if shown.sighted == Some(tag.0) {
            (Color::WHITE, 2.)
        } else if up == Some(tag.0) {
            (srgb(MUSTARD), 2.)
        } else {
            (Color::srgba(own[0], own[1], own[2], 0.85), 1.)
        };
        if *edge != BorderColor::all(rim) {
            *edge = BorderColor::all(rim);
        }
        if node.border != UiRect::all(Val::Px(width)) {
            node.border = UiRect::all(Val::Px(width));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::cards::{Card, Kind, SeatView, Status};

    #[test]
    fn the_seat_in_sight_is_the_nearest_within_the_angle() {
        let angle = |degrees: f32| Quat::from_rotation_y(degrees.to_radians()) * Vec3::NEG_Z;
        let eye = Vec3::new(1., 2., 3.);
        let seats = [eye + angle(0.) * 0.3, eye + angle(18.) * 0.2, eye + angle(-60.) * 0.25];
        // Straight at a seat: that one.
        assert_eq!(seat_in_sight(eye, angle(0.), &seats, 25_f32.to_radians()), Some(0));
        // Between two within the angle: the nearer to where the eye looks.
        assert_eq!(seat_in_sight(eye, angle(12.), &seats, 25_f32.to_radians()), Some(1));
        assert_eq!(seat_in_sight(eye, angle(-55.), &seats, 25_f32.to_radians()), Some(2));
        // 40 degrees off the only one: none.
        assert_eq!(seat_in_sight(eye, angle(40.), &seats[..1], 25_f32.to_radians()), None);
        // A seat left out (not a place: one that cannot be picked) is never in sight.
        let out = [Vec3::NAN, seats[1]];
        assert_eq!(seat_in_sight(eye, angle(0.), &out, 25_f32.to_radians()), Some(1));
        assert_eq!(seat_in_sight(eye, angle(0.), &[Vec3::NAN], 25_f32.to_radians()), None);
        // Looking up or down at one counts the same as across.
        let above = [eye + (Quat::from_rotation_x(0.3) * Vec3::NEG_Z) * 0.3];
        assert_eq!(seat_in_sight(eye, Vec3::NEG_Z, &above, 25_f32.to_radians()), Some(0));
    }

    /// A table of five in the middle of a hand: seat 1 bust on a second Ketchup, seat 2 to flip or stop (its go),
    /// seat 3 stopped, seat 4 fridged with a Napkin held; totals from earlier hands.
    fn mid_hand() -> View {
        let seat = |cards: Vec<Card>, status: Status, points: u32, total: u32| SeatView { napkin: cards.contains(&Card::Napkin), cards, status, points, total, bot: true };
        View {
            seats: vec![
                seat(vec![Card::Topping(8), Card::Side(2)], Status::In, 10, 42),
                seat(vec![Card::Topping(12), Card::Topping(12)], Status::Bust, 0, 30),
                seat(vec![Card::Topping(5)], Status::In, 5, 51),
                seat(vec![Card::Topping(4), Card::Topping(9)], Status::Stopped, 13, 18),
                seat(vec![Card::Napkin, Card::Topping(11)], Status::Fridged, 11, 67),
            ],
            dealer: 1,
            hand: 3,
            turn: Some(2),
            choosing: None,
            clock: None,
            deck: 60,
            between: false,
            winner: None,
            events: vec![],
            version: 9,
        }
    }
    fn names() -> Vec<(String, [f32; 3])> {
        ["You", "Relish", "Frank", "Bot 1", "Bot 2"].iter().enumerate().map(|(i, n)| (n.to_string(), kitchen_sim::feast::COLORS[i])).collect()
    }

    #[test]
    fn a_seat_is_named_in_the_colour_its_player_chose() {
        let roster = [Player { id: 1, name: "Ann".into(), ..default() }, Player { id: 4, name: "MUSTARD".into(), ..default() }];
        let names = super::names(&roster, |id| (id == 1).then_some([0.07, 0.30, 0.90]));
        assert_eq!(names, vec![("Ann".to_string(), [0.07, 0.30, 0.90]), ("MUSTARD".to_string(), kitchen_sim::feast::COLORS[3])]);
    }
    #[test]
    fn the_scoreboard_rows_follow_the_view() {
        let view = mid_hand();
        let rows = rows(&view, &names(), Some(0));
        // A row a seat, in seat order, each named in its colour.
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["You", "Relish", "Frank", "Bot 1", "Bot 2"]);
        assert!(rows.iter().enumerate().all(|(i, r)| r.colour == kitchen_sim::feast::COLORS[i]));
        // The seat whose go it is is highlighted, and only that one; this player's own row is marked as its own.
        assert_eq!(rows.iter().map(|r| r.up).collect::<Vec<_>>(), [false, false, true, false, false]);
        assert_eq!(rows.iter().map(|r| r.me).collect::<Vec<_>>(), [true, false, false, false, false]);
        // The cards by name, the hand's points over the total, and how each stands in the hand.
        assert_eq!(rows[1].cards.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Ketchup 12", "Ketchup 12"]);
        assert_eq!(rows[4].cards.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Napkin", "Mustard 11"]);
        assert_eq!(rows.iter().map(|r| r.score.as_str()).collect::<Vec<_>>(), ["10 / 42", "0 / 30", "5 / 51", "13 / 18", "11 / 67"]);
        assert_eq!(rows.iter().map(|r| r.mark).collect::<Vec<_>>(), ["", "BUST", "", "STOPPED", "FRIDGED"]);
        // Fully loaded, and the winner; a hotdog picking who gets an action card is the one highlighted.
        let mut view = mid_hand();
        view.seats[2].status = Status::Loaded;
        view.turn = None;
        view.choosing = Some((3, Card::PileOn));
        let rows = super::rows(&view, &names(), Some(0));
        assert_eq!(rows[2].mark, "FULLY LOADED");
        assert_eq!(rows.iter().map(|r| r.up).collect::<Vec<_>>(), [false, false, false, true, false]);
        view.winner = Some(4);
        view.choosing = None;
        assert_eq!(super::rows(&view, &names(), Some(0))[4].mark, "WINNER");
        // A seat the roster has not named yet is still a row.
        assert_eq!(super::rows(&mid_hand(), &names()[..3], None).len(), 5);
    }

    #[test]
    fn name_tags_keep_clear_of_the_scoreboard() {
        let panel = Rect::new(16., 16., 520., 170.);
        let size = Vec2::new(64., 20.);
        // A tag whose head is under the panel goes below it, still over the head across.
        let at = clear_of(Vec2::new(120., 90.), size, panel);
        assert!(at.y >= panel.max.y, "{at}");
        assert_eq!(at.x, 120. - size.x / 2.);
        // Only just poking into it (its head just below, its tag up into the panel's foot, or its corner): moved too.
        assert!(clear_of(Vec2::new(300., panel.max.y + 10.), size, panel).y >= panel.max.y);
        assert!(clear_of(Vec2::new(panel.max.x + 20., 100.), size, panel).y >= panel.max.y);
        // Clear of it already: where it was, over the head.
        let spot = Vec2::new(800., 90.);
        assert_eq!(clear_of(spot, size, panel), Vec2::new(800. - size.x / 2., 90. - GAP - size.y));
        let below = Vec2::new(200., 400.);
        assert_eq!(clear_of(below, size, panel), Vec2::new(200. - size.x / 2., 400. - GAP - size.y));
    }

    /// The deal the tests play (a game by the clock is a different one every time: `fresh_seed`). In this one player 1
    /// draws Pile It On before its first flip or stop, and so has a hotdog to pick; in `FRIDGE`, Back in the Fridge.
    const SEED: u64 = 0;
    const FRIDGE: u64 = 5;

    /// Single player's game by `rules`, with this module, played frame by frame; its first frames have begun the
    /// round. A card table is laid again from `seed` after the first (which shuffled it by the clock, as a game does),
    /// so that a test plays the same game every time.
    fn game(rules: &str, seed: u64) -> App {
        let mut app = crate::arena::test_app(rules);
        app.add_plugins(CardsPlugin).add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            if g.feast.cards.is_some() {
                g.feast.card_seed = seed;
                let rules = g.feast.rules.clone();
                g.feast.set_rules(rules);
            }
        }
        crate::arena::test_frame(&mut app, 0.05);
        app
    }
    fn view(app: &App) -> Option<View> {
        app.world().resource::<Seen>().view.clone()
    }
    fn last(v: &View) -> u64 {
        v.events.iter().map(|e| e.seq).max().unwrap_or(0)
    }
    /// Plays on until the table waits on player 1 to flip or stop (single player has no limit: once its card has
    /// shown, nothing moves until it decides) and returns that view. Whenever player 1 is to pick who an action card it
    /// drew is played on (which waits on it too) it picks through `Wishes`: the first other hotdog still in, or itself.
    /// The picks are returned too, each checked to have been played.
    fn play_to_player_1s_turn(app: &mut App) -> (View, Vec<(Card, usize)>) {
        let mut picks = vec![];
        // (Two minutes of play: a hotdog fridged or ruined in the first hand waits for the second.)
        for _ in 0..1200 {
            crate::arena::test_frame(app, 0.1);
            let v = view(app).unwrap();
            if let Some((0, card)) = v.choosing {
                let at = (1..v.seats.len()).chain([0]).find(|s| v.seats[*s].status == Status::In).expect("a hotdog still in");
                app.world_mut().resource_mut::<Wishes>().0.push(Command::Target(at));
                crate::arena::test_frame(app, 0.01);
                assert!(app.world().resource::<Wishes>().0.is_empty(), "the pick is taken");
                let after = view(app).unwrap();
                let played = after.events.iter().any(|e| e.seq > last(&v) && e.seat == 0 && matches!(e.kind, Kind::Fridged | Kind::PiledOn) && e.target == Some(at));
                assert!(played, "{card:?} on {at}: {:?}", after.events);
                picks.push((card, at));
                continue;
            }
            if v.turn != Some(0) || v.choosing.is_some() {
                continue;
            }
            for _ in 0..15 {
                crate::arena::test_frame(app, 0.1);
            }
            let still = view(app).unwrap();
            if still.version == v.version && still.turn == Some(0) && still.choosing.is_none() {
                return (still, picks);
            }
        }
        panic!("player 1's turn never came");
    }

    #[test]
    fn seen_follows_the_offline_table_and_clears_when_the_rules_change() {
        let mut app = game("mode=cards", SEED);
        assert!(at_table(app.world().resource::<Game>()));
        let first = view(&app).expect("a table to see");
        assert_eq!(first.seats.len(), 5);
        assert_eq!(app.world().resource::<Seen>().me, Some(0), "player 1 sits at the first seat");
        // The table plays on by itself (the bots) and what is seen follows it, frame by frame.
        for _ in 0..80 {
            crate::arena::test_frame(&mut app, 0.1);
            let g = app.world().resource::<Game>();
            assert_eq!(view(&app), g.feast.cards.as_ref().map(|t| t.view()));
        }
        assert!(view(&app).unwrap().version > first.version, "the offline table advances");
        // A game that is no longer cards: nothing is seen, and no seat is this player's.
        app.world_mut().resource_mut::<Game>().feast.set_rules(kitchen_sim::rules::Rules::default());
        crate::arena::test_frame(&mut app, 0.05);
        assert!(!at_table(app.world().resource::<Game>()));
        assert!(view(&app).is_none() && app.world().resource::<Seen>().me.is_none());
        // Nor in a siege from the start.
        let siege = game("", SEED);
        assert!(view(&siege).is_none() && !at_table(siege.world().resource::<Game>()));
    }

    /// The offline demo (`?demo`, `--demo`) of Fully Loaded watches the table: every hotdog at it is a bot's, so no seat
    /// is this player's, and it is shown no prompt to flip, stop or pick and has no buttons that ask anything of the
    /// table (it must not be able to play seat 0's bot).
    #[test]
    fn the_demo_watches_the_table_and_sits_at_no_seat() {
        let mut app = game("mode=cards", SEED);
        assert_eq!(app.world().resource::<Seen>().me, Some(0), "a game of one's own sits at the first seat");
        {
            // What the demo does at its start: every hotdog a bot, the table laid afresh.
            let mut g = app.world_mut().resource_mut::<Game>();
            g.demo = true;
            g.begin();
            // (This deal has seat 0 pick who gets Pile It On before its first go.)
            g.feast.card_seed = SEED;
            let rules = g.feast.rules.clone();
            g.feast.set_rules(rules);
        }
        assert!(app.world().resource::<Game>().feast.players.iter().all(|p| p.bot));
        let names = names();
        let (mut turns, mut picks) = (0, 0);
        for _ in 0..1200 {
            crate::arena::test_frame(&mut app, 0.1);
            let g = app.world().resource::<Game>();
            if !at_table(g) {
                continue;
            }
            let (view, me) = (view(&app).expect("a table to watch"), app.world().resource::<Seen>().me);
            assert_eq!(me, None, "the demo's viewer sits at no seat");
            assert_eq!(asks(&view, me), (false, false), "nothing is asked of it");
            let line = prompt(&view, &names, me, None, false);
            let asked = line.iter().any(|piece| matches!(piece, Piece::Icon(Icon::MouseLeft | Icon::MouseRight | Icon::Target, _)));
            assert!(!asked, "no prompt to play: {line:?}");
            // (It is a bot's go and pick that came, and went by itself.)
            turns += (view.turn == Some(0)) as u32;
            picks += view.choosing.is_some_and(|(seat, _)| seat == 0) as u32;
        }
        assert!(turns > 0 && picks > 0, "seat 0 had its go ({turns}) and its pick ({picks}) with nobody to play them");
        assert_eq!(crate::touch::state(app.world().resource::<Game>(), false, app.world().get_resource::<Seen>()), 0, "and no touch buttons");
    }

    #[test]
    fn wishes_reach_the_offline_table() {
        // Back in the Fridge picked for, then a flip.
        let mut app = game("mode=cards", FRIDGE);
        let (before, picks) = play_to_player_1s_turn(&mut app);
        assert_eq!(picks.iter().map(|(card, _)| *card).collect::<Vec<_>>(), [Card::Fridge], "the pinned deal has player 1 pick");
        app.world_mut().resource_mut::<Wishes>().0.push(Command::Flip);
        crate::arena::test_frame(&mut app, 0.01);
        let after = view(&app).unwrap();
        assert!(after.events.iter().any(|e| e.seq > last(&before) && e.seat == 0 && e.kind == Kind::Flipped), "{:?}", after.events);
        // Pile It On picked for, then a flip; and a flip out of turn first.
        let mut app = game("mode=cards", SEED);
        // A flip before the table asks player 1 (the first hand's dealer, the last to go) is refused, and taken all the
        // same: nothing waits for later.
        let v = view(&app).unwrap();
        assert_ne!(v.turn, Some(0));
        app.world_mut().resource_mut::<Wishes>().0.push(Command::Flip);
        crate::arena::test_frame(&mut app, 0.01);
        assert!(app.world().resource::<Wishes>().0.is_empty());
        assert!(!view(&app).unwrap().events.iter().any(|e| e.seat == 0 && e.kind == Kind::Flipped));
        // On the way to its turn, this deal has player 1 pick who an action card it drew is played on: the pick goes
        // through `Wishes` too.
        let (before, picks) = play_to_player_1s_turn(&mut app);
        assert_eq!(picks.iter().map(|(card, _)| *card).collect::<Vec<_>>(), [Card::PileOn], "the pinned deal has player 1 pick");
        app.world_mut().resource_mut::<Wishes>().0.push(Command::Flip);
        crate::arena::test_frame(&mut app, 0.01);
        assert!(app.world().resource::<Wishes>().0.is_empty(), "the wish is taken");
        let after = view(&app).unwrap();
        assert!(after.version > before.version);
        assert!(after.events.iter().any(|e| e.seq > last(&before) && e.seat == 0 && e.kind == Kind::Flipped), "{:?}", after.events);
        // A wish away from the table goes nowhere.
        app.world_mut().resource_mut::<Game>().feast.set_rules(kitchen_sim::rules::Rules::default());
        app.world_mut().resource_mut::<Wishes>().0.push(Command::Stop);
        crate::arena::test_frame(&mut app, 0.01);
        assert!(app.world().resource::<Wishes>().0.is_empty());
    }

    #[test]
    fn wishes_are_written_as_the_server_reads_them() {
        assert_eq!(wish(Command::Flip), serde_json::json!({"type": "cards", "action": "flip"}));
        assert_eq!(wish(Command::Stop), serde_json::json!({"type": "cards", "action": "stop"}));
        assert_eq!(wish(Command::Target(3)), serde_json::json!({"type": "cards", "action": "target", "seat": 3}));
    }

    #[test]
    fn every_offline_game_is_shuffled_afresh() {
        let seeds: Vec<u64> = (0..3)
            .map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(2));
                fresh_seed()
            })
            .collect();
        assert!(seeds[0] != seeds[1] && seeds[1] != seeds[2], "{seeds:?}");
        // Two games begun one after the other (as the arena begins them, by the clock) have different seeds, so
        // different decks.
        let mut app = crate::arena::test_app("mode=cards");
        app.add_plugins(CardsPlugin).add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        let first = app.world().resource::<Game>().feast.card_seed;
        std::thread::sleep(std::time::Duration::from_millis(2));
        app.world_mut().resource_mut::<Game>().begin();
        assert_ne!(app.world().resource::<Game>().feast.card_seed, first);
        assert!(app.world().resource::<Game>().feast.cards.is_some());
    }

    #[test]
    fn the_prompt_says_what_to_do() {
        use Piece::Icon as I;
        let names = names();
        // The words of a prompt, its icons as [Name] (none but names and numbers are words).
        let text = |line: Vec<Piece>| {
            line.into_iter()
                .map(|piece| match piece {
                    Piece::Text(words, _) => words,
                    I(icon, _) => format!("[{icon:?}]"),
                })
                .collect::<String>()
        };
        let mut view = mid_hand();
        // On this player's go: the left button and a card (flip), the right button and a hand (stop), and the seconds
        // left when there is a limit (up to the whole second).
        view.turn = Some(0);
        assert_eq!(text(prompt(&view, &names, Some(0), None, false)), "[MouseLeft][Card]   [MouseRight][Hand]");
        assert_eq!(text(prompt(&view, &names, Some(0), Some(11.3), false)), "[MouseLeft][Card]   [MouseRight][Hand]   [Clock]12");
        // With the touch controls showing, no mouse buttons: the screen's Flip and Stop say it.
        assert_eq!(text(prompt(&view, &names, Some(0), Some(11.3), true)), "[Card]   [Hand]   [Clock]12");
        // Picking who gets an action card: a target and the card (in its colour), and the button.
        view.choosing = Some((0, Card::PileOn));
        assert_eq!(text(prompt(&view, &names, Some(0), None, false)), "[Target]Pile It On[MouseLeft]");
        assert_eq!(text(prompt(&view, &names, Some(0), None, true)), "[Target]Pile It On");
        // Another's pick or go: whom it waits on, named in its colour.
        view.choosing = Some((3, Card::Fridge));
        let line = prompt(&view, &names, Some(0), Some(9.), false);
        assert_eq!(text(line.clone()), "Bot 1 ...");
        assert_eq!(prompt(&view, &names, Some(0), Some(9.), true), line, "the rest is the same by touch");
        assert_eq!(line[0], Piece::Text("Bot 1".into(), Some(lighter(names[3].1, 0.3))));
        (view.choosing, view.turn) = (None, Some(2));
        assert_eq!(text(prompt(&view, &names, Some(0), None, false)), "Frank ...");
        (view.turn, view.dealer) = (None, 0);
        assert_eq!(text(prompt(&view, &names, Some(0), None, false)), "[Card] ...");
        // Between hands: each seat's points this hand, a bust's as BUST.
        view.between = true;
        assert_eq!(text(prompt(&view, &names, Some(0), None, false)), "You +10   Relish BUST   Frank +5   Bot 1 +13   Bot 2 +11");
        // The winner: a crown, the name and the total.
        view.winner = Some(4);
        assert_eq!(text(prompt(&view, &names, Some(0), None, false)), "[Crown]Bot 2 67");
        // No sentences: every word is a name, a card's name, a number or BUST.
        // The screen's font has ASCII only.
        assert_eq!(plain("Jalapeños 3, Foot-Long ×2, thinking…"), "Jalapenos 3, Foot-Long x2, thinking...");
        // The scoreboard's chips: a value, or an action card's icon.
        assert_eq!(chip(Card::Topping(12)), Ok("12".into()));
        assert_eq!(chip(Card::Side(2)), Ok("+2".into()));
        assert_eq!(chip(Card::FootLong), Ok("x2".into()));
        assert_eq!([chip(Card::Fridge), chip(Card::PileOn), chip(Card::Napkin)], [Err(Icon::Fridge), Err(Icon::Pile), Err(Icon::Drape)]);
    }

    #[test]
    fn the_hotdogs_at_the_table_hold_no_gun_and_look_at_whoever_plays() {
        // Posed at the table: no gun, a slump only after a bust, a cheer only when fully loaded or the winner; away from
        // the table as the round has it.
        let round = Cues { alive: true, cheer: true, weapon: 1, crouch: true, pitch: 0.2, ..default() };
        let (yaw, cues) = dress(Some(Posed { yaw: 1.2, cheer: false, slump: false }), 0.3, round);
        assert_eq!((yaw, cues.weapon, cues.cheer, cues.crouch, cues.alive, cues.pitch), (1.2, 0, false, false, true, 0.2));
        let (_, cues) = dress(Some(Posed { yaw: 1.2, cheer: true, slump: true }), 0.3, round);
        assert!(cues.cheer && cues.crouch && cues.weapon == 0);
        let (yaw, cues) = dress(None, 0.3, round);
        assert_eq!((yaw, cues.weapon, cues.cheer, cues.crouch, cues.alive), (0.3, 0, false, false, true));
        let (yaw, cues) = seated(&World::new(), false, 1, 0.3, round);
        assert_eq!((yaw, cues.weapon, cues.cheer, cues.crouch), (0.3, 1, true, true));
        // Heads turn to whoever's go or pick it is; that hotdog, and everybody while it is nobody's go, faces the middle.
        let level = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::bundled(), false).level;
        let spot = Spot::new(&level, 5).unwrap();
        let towards = |view: &View, seat: usize| {
            for (i, yaw) in heads(view, &spot.seats).into_iter().enumerate() {
                let want = if i == seat { spot.seats[i].1 } else { facing(spot.seats[i].0, spot.seats[seat].0) };
                assert!((yaw - want).abs() < 1e-5, "seat {i} looks at {seat}");
            }
        };
        let mut view = mid_hand();
        towards(&view, 2);
        (view.turn, view.choosing) = (None, Some((4, Card::Fridge)));
        towards(&view, 4);
        view.choosing = None;
        assert_eq!(heads(&view, &spot.seats), spot.seats.iter().map(|(_, yaw)| *yaw).collect::<Vec<_>>());
        // (A yaw that faces the middle faces it: the seats' own.)
        assert!((facing(spot.seats[1].0, spot.middle) - spot.seats[1].1).abs() < 1e-4);
        // Cheering: fully loaded, or the winner.
        view.seats[1].status = Status::Loaded;
        assert!(cheers(&view, 1) && !cheers(&view, 0) && !cheers(&view, 3));
        view.winner = Some(3);
        assert!(cheers(&view, 3));
        // A bust slumps its hotdog from when it is heard of; a table's events seen first (a join, a new game) are old news.
        let bust = |seq: u64, seat: usize| kitchen_sim::cards::Event { seq, seat, kind: Kind::Bust, card: Some(Card::Topping(3)), target: None };
        let mut poses = Poses::default();
        let mut view = mid_hand();
        view.events = vec![bust(5, 1)];
        poses.hear(&view, 10.);
        assert!(poses.busts.is_empty());
        view.events.push(bust(6, 3));
        poses.hear(&view, 11.);
        assert_eq!(poses.busts, HashMap::from([(3, 11.)]));
        poses.hear(&view, 12.);
        assert_eq!(poses.busts, HashMap::from([(3, 11.)]));
        view.events = vec![bust(1, 0)];
        poses.hear(&view, 13.);
        assert!(poses.busts.is_empty());

        // In a game: at player 1's go the others all turn to it, nobody holds a gun, and its own hotdog is not drawn.
        let mut app = game("mode=cards", SEED);
        play_to_player_1s_turn(&mut app);
        let ids: Vec<u32> = app.world().resource::<Game>().roster.iter().map(|p| p.id).collect();
        let world = app.world_mut();
        let bodies: Vec<(u32, u32, Quat, Visibility)> = world
            .query_filtered::<(&crate::player_model::Gait, &Transform, &Visibility), With<crate::player_model::PlayerBody>>()
            .iter(world)
            .map(|(gait, at, shown)| (gait.id(), gait.motion().weapon as u32, at.rotation, *shown))
            .collect();
        assert_eq!(bodies.len(), 5);
        for (id, weapon, turn, shown) in bodies {
            let seat = ids.iter().position(|i| *i == id).unwrap();
            assert_eq!(weapon, 0, "hotdog {id} holds no gun at the table");
            if seat == 0 {
                assert_eq!(shown, Visibility::Hidden, "this player's own hotdog is not drawn");
            } else {
                let want = Quat::from_rotation_y(facing(spot.seats[seat].0, spot.seats[0].0));
                assert!(turn.angle_between(want) < 0.02, "hotdog {id} looks at player 1: {}", turn.angle_between(want));
            }
        }
        // Back in a siege, the round's hotdogs hold their guns again.
        app.world_mut().resource_mut::<Game>().feast.set_rules(kitchen_sim::rules::Rules::default());
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        let world = app.world_mut();
        let weapons: Vec<usize> = world.query::<&crate::player_model::Gait>().iter(world).map(|gait| gait.motion().weapon).collect();
        assert!(weapons.iter().all(|w| *w > 0), "{weapons:?}");
    }

    fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
        app.world_mut().query_filtered::<(), F>().iter(app.world()).count()
    }
    fn lens(app: &mut App) -> (Transform, f32) {
        let world = app.world_mut();
        let (at, projection) = world.query_filtered::<(&Transform, &Projection), With<HotdogCamera>>().single(world).unwrap();
        let Projection::Perspective(lens) = projection else { panic!("a perspective camera") };
        (*at, lens.fov)
    }

    #[test]
    fn the_table_is_drawn_while_it_is_played_and_all_of_it_goes_after() {
        let mut app = game("mode=cards", SEED);
        let (view, _) = play_to_player_1s_turn(&mut app);
        // The table: its foot, leg, rim and felt.
        assert_eq!(count::<(With<Laid>, With<Mesh3d>, Without<Dealt>)>(&mut app), 4);
        // Every card in front of every seat, the draw pile (a block and the back on it), and the mark of whose go it is.
        let cards: usize = view.seats.iter().map(|s| s.cards.len()).sum();
        assert!(cards >= 5);
        assert_eq!(count::<With<Dealt>>(&mut app), cards + 2 + 1);
        // The scoreboard's rows and the name tags, a seat each.
        assert_eq!(count::<With<BoardRow>>(&mut app), 5);
        assert_eq!(count::<With<Tag>>(&mut app), 5);
        // The eye at this player's seat, looking at the table's middle (a little over the felt).
        let spot = Spot::new(&app.world().resource::<Game>().feast.level, 5).unwrap();
        let (at, fov) = lens(&mut app);
        assert!(at.translation.distance(spot.eye(0)) < 1e-4 && (fov - FOV.to_radians()).abs() < 1e-6, "{at:?} {fov}");
        assert!((spot.eye(0).y - spot.middle.y - EYE).abs() < 1e-5);
        let ahead = at.forward();
        let middle = (spot.felt() + Vec3::Y * AIM - at.translation).normalize();
        assert!(ahead.angle_between(middle) < 1e-3);
        // The cards lie on the felt, each upright to the eye, and the seats' cards apart from each other's: each nearer
        // its own hotdog than any other.
        let world = app.world_mut();
        let laid: Vec<Vec3> = world.query_filtered::<&Transform, (With<Dealt>, With<Mesh3d>)>().iter(world).map(|at| at.translation).collect();
        assert!(laid.iter().all(|at| spot.on(*at, 0.) && at.y > spot.felt().y && at.y < spot.felt().y + 0.012), "{laid:?}");
        for (seat, s) in view.seats.iter().enumerate() {
            for (card, at) in spread(&spot, spot.eye(0), seat, &s.cards) {
                let nearest = (0..5).min_by(|a, b| at.translation.distance(spot.seats[*a].0).total_cmp(&at.translation.distance(spot.seats[*b].0))).unwrap();
                assert_eq!(nearest, seat, "a card of seat {seat} at {}", at.translation);
                // (A Napkin held lies turned a little.)
                let eye = spot.eye(0) - at.translation;
                let off = (at.rotation * Vec3::Z).angle_between(Vec3::new(eye.x, 0., eye.z));
                assert!(off < if card == Card::Napkin { 0.65 } else { 0.35 }, "{card:?} of seat {seat} is {off} off upright");
            }
        }
        // Every kind of card was printed as the table was first laid; and this player's cards are those on the table in
        // front of it (it flips one, should it have none yet), none of them shown again on the screen.
        if view.seats[0].cards.is_empty() {
            app.world_mut().resource_mut::<Wishes>().0.push(Command::Flip);
            for _ in 0..20 {
                crate::arena::test_frame(&mut app, 0.1);
            }
        }
        let mine = super::tests::view(&app).unwrap().seats[0].cards.len();
        assert!(mine > 0);
        let world = app.world_mut();
        let prints: Vec<AssetId<Image>> = {
            let (shown, materials) = (world.resource::<Shown>(), world.resource::<Assets<StandardMaterial>>());
            shown.kit.as_ref().unwrap().faces.values().filter_map(|m| Some(materials.get(m)?.base_color_texture.as_ref()?.id())).collect()
        };
        assert_eq!(prints.len(), 13 + 5 + 1 + 3);
        assert_eq!(world.query::<&ImageNode>().iter(world).filter(|node| prints.contains(&node.image.id())).count(), 0, "{mine} cards of this player's on the screen");
        // Back in a siege: nothing of the table is left, and the camera is the round's again.
        app.world_mut().resource_mut::<Game>().feast.set_rules(kitchen_sim::rules::Rules::default());
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        assert_eq!(count::<With<Laid>>(&mut app), 0);
        assert_eq!(count::<Or<(With<Dealt>, With<BoardRow>, With<Tag>, With<Prompt>, With<Board>)>>(&mut app), 0);
        let (at, fov) = lens(&mut app);
        let g = app.world().resource::<Game>();
        assert!((fov - 85_f32.to_radians()).abs() < 1e-5 && at.translation.distance(g.position) < 0.2, "{at:?} {fov}");
        assert!(app.world().resource::<Poses>().by.is_empty());
    }

    /// The kinds of card are printed a few a frame from when the table is first laid, and the table is dealt once all
    /// of them are: no deal waits on a print, nor any frame on all 22.
    #[test]
    fn the_cards_are_printed_a_few_a_frame_and_dealt_once_all_are() {
        let (mut meshes, mut materials, mut images) = (Assets::<Mesh>::default(), Assets::<StandardMaterial>::default(), Assets::<Image>::default());
        let mut kit = Kit::new(&mut meshes, &mut materials, &mut images);
        assert!(kit.faces.is_empty());
        let mut printed = vec![];
        while !kit.print(&mut materials, &mut images) {
            printed.push(kit.faces.len());
        }
        printed.push(kit.faces.len());
        assert_eq!(printed, [4, 8, 12, 16, 20, 22]);
        // In a game, frame by frame from the first: how many kinds are printed, and whether the table is dealt.
        let mut app = crate::arena::test_app("mode=cards");
        app.add_plugins(CardsPlugin).add_systems(Update, crate::arena::tick);
        let mut frames = vec![];
        for _ in 0..8 {
            crate::arena::test_frame(&mut app, 0.05);
            let printed = app.world().resource::<Shown>().kit.as_ref().map_or(0, |kit| kit.faces.len());
            frames.push((printed, count::<With<Dealt>>(&mut app) > 0));
        }
        assert_eq!(frames, [(4, false), (8, false), (12, false), (16, false), (20, false), (22, true), (22, true), (22, true)]);
    }

    /// Plays on until player 1 is to pick who gets an action card it drew, and returns that view.
    fn play_to_player_1s_pick(app: &mut App) -> View {
        for _ in 0..600 {
            crate::arena::test_frame(app, 0.1);
            let v = view(app).unwrap();
            if v.choosing.is_some_and(|(seat, _)| seat == 0) {
                return v;
            }
        }
        panic!("player 1 never picked");
    }
    fn press(app: &mut App, button: MouseButton) {
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(button);
        crate::arena::test_frame(app, 0.01);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release_all();
    }
    fn window(app: &mut App, change: impl FnOnce(&mut Window, &mut CursorOptions)) {
        let world = app.world_mut();
        let (mut window, mut cursor) = world.query::<(&mut Window, &mut CursorOptions)>().single_mut(world).unwrap();
        change(&mut window, &mut cursor);
    }
    fn captured(app: &mut App) -> bool {
        let world = app.world_mut();
        world.query::<&CursorOptions>().single(world).unwrap().grab_mode == CursorGrabMode::Locked
    }
    fn played(app: &mut App, after: &View, kind: Kind) -> Option<kitchen_sim::cards::Event> {
        view(app).unwrap().events.into_iter().find(|e| e.seq > last(after) && e.seat == 0 && e.kind == kind)
    }

    #[test]
    fn clicks_flip_stop_and_pick_and_the_scoreboard_keeps_its_own() {
        let mut app = game("mode=cards", SEED);
        // This deal has player 1 pick who gets Pile It On before its first go.
        let picking = play_to_player_1s_pick(&mut app);
        assert_eq!(picking.choosing, Some((0, Card::PileOn)));
        let target = (1..5).find(|seat| picking.seats[*seat].status == Status::In).unwrap();
        // The scoreboard where the screen's layout would put it (a test lays nothing out): the panel, and the row of a
        // hotdog still in the hand.
        {
            let world = app.world_mut();
            let laid = |w: f32, h: f32| ComputedNode { size: Vec2::new(w, h), inverse_scale_factor: 1., ..default() };
            let panel = world.query_filtered::<Entity, With<Board>>().single(world).unwrap();
            world.entity_mut(panel).insert((laid(300., 200.), UiGlobalTransform::from_xy(164., 114.)));
            let row = world.query::<(Entity, &BoardRow)>().iter(world).find(|(_, row)| row.0 == target).unwrap().0;
            world.entity_mut(row).insert((laid(280., 30.), UiGlobalTransform::from_xy(164., 100.)));
        }
        window(&mut app, |window, cursor| {
            cursor.grab_mode = CursorGrabMode::None;
            window.set_cursor_position(Some(Vec2::new(100., 180.)));
        });
        // A click on the scoreboard but on no row: nothing, and the pointer stays free.
        press(&mut app, MouseButton::Left);
        assert!(!captured(&mut app));
        assert!(played(&mut app, &picking, Kind::PiledOn).is_none());
        // With the window not the keyboard's (the page's chat box open), a click on the row does nothing either.
        window(&mut app, |window, _| {
            window.focused = false;
            window.set_cursor_position(Some(Vec2::new(100., 100.)));
        });
        press(&mut app, MouseButton::Left);
        assert!(played(&mut app, &picking, Kind::PiledOn).is_none());
        // Off the scoreboard, a click with the pointer free only captures it: nobody is picked (`click`).
        window(&mut app, |window, _| {
            window.focused = true;
            window.set_cursor_position(Some(Vec2::new(640., 420.)));
        });
        press(&mut app, MouseButton::Left);
        assert!(captured(&mut app));
        assert!(played(&mut app, &picking, Kind::PiledOn).is_none());
        assert_eq!(view(&app).unwrap().choosing, Some((0, Card::PileOn)), "still picking");
        // Freed again (Esc), a click on the row picks its hotdog, and leaves the pointer free.
        window(&mut app, |window, cursor| {
            cursor.grab_mode = CursorGrabMode::None;
            window.set_cursor_position(Some(Vec2::new(100., 100.)));
        });
        press(&mut app, MouseButton::Left);
        assert_eq!(played(&mut app, &picking, Kind::PiledOn).and_then(|e| e.target), Some(target));
        assert!(!captured(&mut app));
        // At its go, with the pointer free (after Esc, the chat box or another window), a click off the scoreboard only
        // captures the pointer: a flip could ruin the hand. The next click flips.
        let (before, _) = play_to_player_1s_turn(&mut app);
        window(&mut app, |window, cursor| {
            cursor.grab_mode = CursorGrabMode::None;
            window.set_cursor_position(Some(Vec2::new(640., 420.)));
        });
        press(&mut app, MouseButton::Left);
        assert!(captured(&mut app));
        assert!(played(&mut app, &before, Kind::Flipped).is_none(), "the click that captures the pointer flips nothing");
        assert_eq!(view(&app).unwrap().turn, Some(0), "still its go");
        press(&mut app, MouseButton::Left);
        assert!(played(&mut app, &before, Kind::Flipped).is_some());
        // At its next go the right button stops it.
        let (before, _) = play_to_player_1s_turn(&mut app);
        assert!(captured(&mut app));
        press(&mut app, MouseButton::Right);
        assert!(played(&mut app, &before, Kind::Stopped).is_some());
        // The buttons ask nothing of the table when it is not this player's go.
        let v = view(&app).unwrap();
        assert_ne!(v.turn, Some(0));
        press(&mut app, MouseButton::Left);
        press(&mut app, MouseButton::Right);
        assert!(app.world().resource::<Wishes>().0.is_empty());
        assert!(played(&mut app, &v, Kind::Flipped).is_none() && played(&mut app, &v, Kind::Stopped).is_none());
    }

    /// The touch controls at the table (touch.rs): a drag turns the view, the trigger (Pick) picks the hotdog looked at
    /// and (Flip) flips at this player's go, and the scope (Stop, which touch.rs presses as the right button) stops it,
    /// whether or not the game has the keyboard. None of it captures the pointer, and none of it does anything when it
    /// is not this player's go or pick.
    #[test]
    fn the_touch_buttons_flip_stop_and_pick() {
        let mut app = game("mode=cards", SEED);
        // A camera that renders, so that what it looks at can be picked; its place is copied from where the seat puts it
        // (a test propagates no transforms).
        let place = |app: &mut App| {
            let world = app.world_mut();
            for (at, mut global) in world.query_filtered::<(&Transform, &mut GlobalTransform), With<HotdogCamera>>().iter_mut(world) {
                *global = GlobalTransform::from(*at);
            }
        };
        {
            let world = app.world_mut();
            let camera = world.query_filtered::<Entity, With<HotdogCamera>>().single(world).unwrap();
            world.entity_mut(camera).insert((Camera::default(), GlobalTransform::default()));
        }
        // A frame of the touch controls showing: the trigger pulled or not, the scope pressed or not, the view dragged
        // by `drag` (mouse counts, as the page gives them).
        let touch = |app: &mut App, pulled: bool, scope: bool, drag: Vec2| {
            let mut pad = crate::touch::Pad::default();
            (pad.on, pad.pulled, pad.look) = (true, pulled, drag);
            app.world_mut().insert_resource(pad);
            if scope {
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            }
            crate::arena::test_frame(app, 0.01);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release_all();
            let mut pad = crate::touch::Pad::default();
            pad.on = true;
            app.world_mut().insert_resource(pad);
            place(app);
        };
        // As on a phone, the game does not have the keyboard (the page's boxes have had it), and a pointer is somewhere.
        window(&mut app, |window, cursor| {
            cursor.grab_mode = CursorGrabMode::None;
            window.focused = false;
            window.set_cursor_position(Some(Vec2::new(100., 100.)));
        });
        // This deal has player 1 pick who gets Pile It On before its first go: it looks at a hotdog still in by dragging,
        // and pulls the trigger.
        let picking = play_to_player_1s_pick(&mut app);
        assert_eq!(picking.choosing, Some((0, Card::PileOn)));
        touch(&mut app, false, false, Vec2::ZERO);
        let spot = Spot::new(&app.world().resource::<Game>().feast.level, 5).unwrap();
        let (eye, middle) = (spot.eye(0), spot.felt() + Vec3::Y * AIM);
        let angles = |to: Vec3| Vec2::new(f32::atan2(-to.x, -to.z), to.y.atan2(Vec2::new(to.x, to.z).length()));
        let turn = |seat: usize| {
            let d = angles(spot.seats[seat].0 + Vec3::Y * CHEST - eye) - angles(middle - eye);
            Vec2::new((d.x + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI, d.y)
        };
        let target = (1..5).filter(|seat| picking.seats[*seat].status == Status::In).min_by(|a, b| turn(*a).length().total_cmp(&turn(*b).length())).unwrap();
        let before = lens(&mut app).0.rotation;
        touch(&mut app, false, false, -turn(target) / SENSITIVITY);
        assert!(lens(&mut app).0.rotation.angle_between(before) > 0.05, "a drag turns the view");
        assert!((app.world().resource::<Shown>().look - turn(target)).length() < 1e-4, "within the free look's reach");
        touch(&mut app, true, false, Vec2::ZERO);
        assert_eq!(played(&mut app, &picking, Kind::PiledOn).and_then(|e| e.target), Some(target));
        assert!(!captured(&mut app));
        // At its go the trigger flips, and at its next the scope stops.
        let (before, _) = play_to_player_1s_turn(&mut app);
        touch(&mut app, true, false, Vec2::ZERO);
        assert!(played(&mut app, &before, Kind::Flipped).is_some());
        let (before, _) = play_to_player_1s_turn(&mut app);
        touch(&mut app, false, true, Vec2::ZERO);
        assert!(played(&mut app, &before, Kind::Stopped).is_some());
        assert!(!captured(&mut app));
        // Not its go: nothing.
        let v = view(&app).unwrap();
        assert_ne!(v.turn, Some(0));
        touch(&mut app, true, true, Vec2::ZERO);
        assert!(app.world().resource::<Wishes>().0.is_empty());
        assert!(played(&mut app, &v, Kind::Flipped).is_none() && played(&mut app, &v, Kind::Stopped).is_none());
    }

    /// Picking who gets an action card, the touch controls' Stop reads "Me" (touch.js): there is no pointer to click
    /// the scoreboard's own row and no hotdog to look at in one's own seat, so it is the one way by touch to give the
    /// card to oneself. The mouse's right button does nothing while picking, as before; and at the go it still stops.
    #[test]
    fn the_touch_stop_button_picks_this_player_while_it_picks() {
        // The press, as a function of what the table asks and whether the touch controls show.
        assert_eq!(scope(true, false, false, 2), Some(Command::Stop), "its go: stop, by mouse");
        assert_eq!(scope(true, false, true, 2), Some(Command::Stop), "its go: stop, by touch");
        assert_eq!(scope(false, true, true, 2), Some(Command::Target(2)), "picking, by touch: me");
        assert_eq!(scope(false, true, false, 2), None, "picking, by mouse: nothing (a click on a row, or a hotdog in sight, picks)");
        assert_eq!(scope(false, false, true, 2), None, "not its go or its pick");
        assert_eq!(scope(false, false, false, 2), None);

        let mut app = game("mode=cards", SEED);
        window(&mut app, |window, cursor| {
            cursor.grab_mode = CursorGrabMode::None;
            window.focused = false;
            window.set_cursor_position(Some(Vec2::new(100., 100.)));
        });
        let picking = play_to_player_1s_pick(&mut app);
        assert_eq!(picking.choosing, Some((0, Card::PileOn)));
        // The mouse's right button while picking: nothing.
        press(&mut app, MouseButton::Right);
        assert!(app.world().resource::<Wishes>().0.is_empty());
        assert_eq!(view(&app).unwrap().choosing, Some((0, Card::PileOn)), "still picking");
        // The touch controls' scope (the right button, touch.rs) while picking: Pile It On on itself.
        let mut pad = crate::touch::Pad::default();
        pad.on = true;
        app.world_mut().insert_resource(pad);
        press(&mut app, MouseButton::Right);
        assert_eq!(played(&mut app, &picking, Kind::PiledOn).and_then(|e| e.target), Some(0), "picked itself");
        assert!(!captured(&mut app));
    }

    /// The map edited and reloaded while the table is up (the editor's live reload, arena.rs `load_map`) with the
    /// microwave moved or turned: the table's furniture is laid again where it now stands, not left behind (it had
    /// been laid again only when the number of seats changed).
    #[test]
    fn the_table_is_laid_again_where_the_microwave_moves_to() {
        use kitchen_sim::map::{Level, Side};
        let mut app = game("mode=cards", SEED);
        // The foot, leg, rim and felt, where they stand and how they are turned.
        let furniture = |app: &mut App| -> Vec<(Vec3, Quat)> {
            let world = app.world_mut();
            let mut parts: Vec<(Vec3, Quat)> = world.query_filtered::<&Transform, (With<Laid>, With<Mesh3d>, Without<Dealt>)>().iter(world).map(|at| (at.translation, at.rotation)).collect();
            parts.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
            parts
        };
        // Edits the map the way the editor does (a new level, as `load_map` has it) and lets the table notice.
        let edit = |app: &mut App, change: &dyn Fn(&mut kitchen_sim::map::Block)| {
            let mut g = app.world_mut().resource_mut::<Game>();
            let mut map = g.feast.level.map.clone();
            map.blocks.iter_mut().filter(|b| b.style == "microwave").for_each(change);
            g.feast.level = Level::new(map);
            for _ in 0..3 {
                crate::arena::test_frame(app, 0.05);
            }
        };
        let before = furniture(&mut app);
        assert_eq!(before.len(), 4);
        let middle = |app: &App| Spot::new(&app.world().resource::<Game>().feast.level, 5).unwrap().middle;
        let first = middle(&app);
        assert!(before.iter().all(|(at, _)| (at.x - first.x).abs() < 1e-4 && (at.z - first.z).abs() < 1e-4));
        // Moved half a metre along the counter: the same four parts, over the microwave's new top.
        edit(&mut app, &|b| (b.rect[0], b.rect[2]) = (b.rect[0] + 0.5, b.rect[2] + 0.5));
        let (moved, now) = (furniture(&mut app), middle(&app));
        assert!((now - first).length() > 0.4, "the microwave moved: {first} to {now}");
        assert_eq!(moved.len(), 4, "laid again, not added to");
        assert!(moved.iter().all(|(at, _)| (at.x - now.x).abs() < 1e-4 && (at.z - now.z).abs() < 1e-4), "{moved:?} round {now}");
        // Turned to the other way round (the table's middle stays): the parts turn with it.
        edit(&mut app, &|b| b.facing = Some(if b.facing == Some(Side::PlusX) { Side::MinusX } else { Side::PlusX }));
        let turned = furniture(&mut app);
        assert_eq!(turned.len(), 4);
        assert!(turned.iter().zip(&moved).all(|((_, a), (_, b))| a.angle_between(*b) > 0.1), "turned with the microwave");
    }

    /// What is drawn of the table online: each seat's name tag (its text and colour), each scoreboard row's name, and
    /// how the cards on the table are turned (to this player's eye).
    fn drawn(app: &mut App) -> (Vec<(String, Color)>, Vec<String>, Vec<Quat>) {
        let world = app.world_mut();
        let mut tags: Vec<(usize, String, Color)> = world.query::<(&Tag, &Text, &TextColor)>().iter(world).map(|(tag, text, colour)| (tag.0, text.0.clone(), colour.0)).collect();
        tags.sort_by_key(|(seat, ..)| *seat);
        let mut rows: Vec<(usize, Entity)> = world.query::<(Entity, &BoardRow)>().iter(world).map(|(e, row)| (row.0, e)).collect();
        rows.sort_by_key(|(seat, _)| *seat);
        // (A row's first line holds its name first.)
        let first = |world: &World, e: Entity| world.get::<Children>(e).and_then(|children| children.first().copied());
        let names = rows.iter().map(|(_, row)| first(world, *row).and_then(|line| first(world, line)).and_then(|name| world.get::<Text>(name)).map_or(String::new(), |text| text.0.clone())).collect();
        let cards = world.query_filtered::<&Transform, (With<Dealt>, With<Mesh3d>)>().iter(world).map(|at| at.rotation).collect();
        (tags.into_iter().map(|(_, name, colour)| (name, colour)).collect(), names, cards)
    }

    /// Online, the names, the colours and this player's seat come with the frames, which may come after the table
    /// they go with (a person who takes a bot's seat mid-hand, or a bot a person's, is named anew; a hotdog this
    /// player does not yet find among the round's has no seat). Each deals the table again (the tags, the scoreboard and
    /// the cards facing this player's eye) without waiting for the table to move.
    #[test]
    fn a_name_a_colour_or_a_seat_heard_late_deals_the_table_again() {
        use crate::network::tests::{connected, hear, told};
        let (mut app, server, _) = connected();
        app.add_systems(Update, crate::arena::tick);
        let mut table = kitchen_sim::cards::Table::new(5, 7);
        table.tick(5.);
        let view = table.view();
        assert!(view.seats.iter().all(|s| !s.cards.is_empty()), "every seat has been dealt");
        // A frame of the game, its hotdogs called what `who` says (and painted as it says), those it says nothing of left
        // out.
        let tick = std::cell::Cell::new(0);
        let frame = |app: &mut App, said: Vec<crate::network::Incoming>, who: &dyn Fn(u32) -> Option<(&'static str, &'static str)>| {
            tick.set(tick.get() + 1);
            let feast = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::bundled(), false);
            let players: Vec<kitchen_sim::net::Seat> = feast
                .players
                .iter()
                .filter_map(|p| {
                    let (name, color) = who(p.id)?;
                    Some(kitchen_sim::net::Seat { player: Player { name: name.into(), ..p.clone() }, color: color.into(), human: p.id == 2 || !color.is_empty(), ..default() })
                })
                .collect();
            let frame = kitchen_sim::net::Frame { kind: "snapshot".into(), tick: tick.get(), round: 1, rules: "mode=cards".into(), stage: "Warmup".into(), players, ..default() };
            let mut said = said;
            said.push(crate::network::Incoming::Text(serde_json::to_string(&frame).unwrap()));
            app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_millis(50));
            hear(app, &server, said);
        };
        // The table, with frames in which this player's hotdog (2) is not there yet and nobody has a name.
        let early = |id: u32| (id != 2).then_some(("", ""));
        frame(&mut app, vec![told(&view)], &early);
        for _ in 0..6 {
            frame(&mut app, vec![], &early);
        }
        assert!(at_table(app.world().resource::<Game>()));
        assert_eq!(app.world().resource::<Seen>().me, None);
        let (tags, rows, cards) = drawn(&mut app);
        assert_eq!((tags.len(), rows.len()), (5, 5));
        assert!(rows.iter().take(4).all(|name| name.is_empty()));
        // Then everybody, named, and hotdog 4's player chose blue; the table has not moved.
        const NAMES: [&str; 5] = ["Ann", "Bun", "Cob", "Dog", "Eel"];
        let named = |id: u32| Some((NAMES[id as usize - 1], if id == 4 { "blue" } else { "" }));
        frame(&mut app, vec![], &named);
        frame(&mut app, vec![], &named);
        assert_eq!(app.world().resource::<Seen>().view.as_ref().map(|v| v.version), Some(view.version));
        assert_eq!(app.world().resource::<Seen>().me, Some(1));
        let (now, rows, again) = drawn(&mut app);
        assert_eq!(now.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), NAMES, "the tags");
        assert_eq!(rows, NAMES, "the scoreboard");
        assert_eq!(again.len(), cards.len());
        assert!(again.iter().zip(&cards).any(|(a, b)| a.angle_between(*b) > 0.1), "the cards turned to this player's eye");
        let blue = srgb(lighter(crate::player_model::tag_rgb("blue").unwrap(), 0.3));
        assert_eq!(now[3].1, blue, "hotdog 4's tag in its player's colour");
        assert!(now.iter().enumerate().all(|(i, (_, colour))| i == 3 || *colour != blue));
    }

    /// A left click off the scoreboard with the pointer free (the first, or the one back after Esc, the chat box,
    /// another window or a lock the browser let go) only captures it: it neither flips nor picks, whatever is in sight.
    /// Captured, it flips at this player's go and picks the hotdog in sight while it picks.
    #[test]
    fn a_click_that_captures_the_pointer_asks_nothing_of_the_table() {
        for go in [false, true] {
            for picking in [false, true] {
                for sighted in [None, Some(2)] {
                    assert_eq!(click(false, go, picking, sighted), (true, None), "free: go {go}, picking {picking}, {sighted:?} in sight");
                }
            }
        }
        assert_eq!(click(true, true, false, None), (false, Some(Command::Flip)));
        assert_eq!(click(true, false, true, Some(2)), (false, Some(Command::Target(2))));
        assert_eq!(click(true, false, true, None), (false, None), "picking, nobody in sight");
        assert_eq!(click(true, false, false, Some(2)), (false, None), "another's go");
    }

    /// The pointer lock as the table finds it. Natively the window's grab is the truth. In a browser the page can lose
    /// the lock without the game hearing Esc (Esc itself, another tab or window): the grab still says locked, but the
    /// pointer is not captured, and the grab is let go so that the next click asks for the lock again (and only does
    /// that). A lock asked for is not lost while it is still coming (the browser grants it a moment later), unless it
    /// never comes.
    #[test]
    fn a_pointer_lock_the_browser_let_go_is_let_go() {
        // Natively (the page's lock is the grab's), or in a browser with the lock: captured; without the grab: not.
        assert_eq!(grip(true, true, false, 0.), (true, false, true));
        assert_eq!(grip(true, true, true, 30.), (true, false, true));
        assert_eq!(grip(false, true, false, 30.), (false, false, false));
        assert_eq!(grip(false, false, true, 30.), (false, false, false));
        // Asked for and not come yet: not captured (a click now flips nothing), and the grab is kept a while.
        assert_eq!(grip(true, false, false, 0.2), (false, false, false));
        // The browser had it and let it go, unheard: not captured, and the grab is let go.
        assert_eq!(grip(true, false, true, 0.2), (false, true, false));
        assert_eq!(grip(true, false, true, 30.), (false, true, false));
        // Asked for and never come (the browser refused it): let go, so that a click asks again.
        assert_eq!(grip(true, false, false, PATIENCE + 0.1), (false, true, false));
    }

    #[test]
    fn at_the_table_the_gun_keys_do_nothing_and_the_microwave_clock_is_dark() {
        for (rules, table) in [("sandbox mode=cards weapons=all ammo=inf", true), ("sandbox weapons=all ammo=inf", false)] {
            let mut app = game(rules, SEED);
            assert_eq!(at_table(app.world().resource::<Game>()), table);
            // The microwave's clock, a segment each (level.rs builds them with the map; here they stand alone).
            for digit in 0..4 {
                for segment in 0..7 {
                    app.world_mut().spawn((crate::arena::MicrowaveDigit(digit, segment), Visibility::Hidden));
                }
            }
            // Every gun owned (the table's hotdogs hold only what they were fitted with), so that the keys have guns to
            // take in hand.
            for w in 1..kitchen_sim::feast::GUNS {
                app.world_mut().resource_mut::<Game>().feast.players[0].owned[w] = true;
            }
            crate::arena::test_frame(&mut app, 0.05);
            let mut weapons = vec![app.world().resource::<Game>().weapon];
            for key in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::KeyY, KeyCode::Digit1] {
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
                crate::arena::test_frame(&mut app, 0.05);
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
                weapons.push(app.world().resource::<Game>().weapon);
            }
            let lit = count::<(With<crate::arena::MicrowaveDigit>, Without<Laid>)>(&mut app);
            let world = app.world_mut();
            let lit_now = world.query::<(&crate::arena::MicrowaveDigit, &Visibility)>().iter(world).filter(|(_, v)| **v == Visibility::Visible).count();
            assert_eq!(lit, 28);
            let changed = weapons.windows(2).any(|pair| pair[0] != pair[1]);
            if table {
                assert!(!changed && lit_now == 0, "{rules}: {weapons:?}, {lit_now} lit");
            } else {
                assert!(changed && lit_now > 0, "{rules}: {weapons:?}, {lit_now} lit");
            }
        }
    }

    // ----- The tutorial.

    use tutorial::Step;

    /// Single player's game by `rules` (`tutorial` among them asks for the lesson), dealt the same every time. The first
    /// frame begins the game (`Game::begin`), which shuffles by the clock (`fresh_seed`, as every game is shuffled), and
    /// lays the lesson, when it is asked for, from that clock seed: so a card seed set before the first frame is undone
    /// in it, and what was dealt was a different game each run, about one in twenty of them (16 of the first 300 seeds)
    /// with a bot's Back in the Fridge landing on this player in the deal, which sat the hand out and made
    /// `until_asked_or_dealt` fail with "the first hand was never dealt", and `deal_pace` measure a bot's thinking
    /// as the pace of the deal. So, as `game` does, the seed is pinned after that frame and the table laid again from
    /// it (and the lesson, at the next frame), whatever the load or the clock.
    fn lesson_game(rules: &str) -> App {
        let mut app = crate::arena::test_app(rules);
        app.add_plugins(CardsPlugin).add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast.card_seed = 3;
            if g.feast.cards.is_some() {
                let rules = g.feast.rules.clone();
                g.feast.set_rules(rules);
            }
        }
        // (The lesson laid in the first frame is forgotten: it is laid afresh at the next, from the pinned seed.)
        *app.world_mut().resource_mut::<Tutorial>() = Tutorial::default();
        crate::arena::test_frame(&mut app, 0.05);
        app
    }
    /// The tests' games are dealt the same every time, with the lesson and without it, and a game by the clock is not
    /// (`lesson_game`).
    #[test]
    fn the_games_the_tests_play_are_dealt_the_same_every_time() {
        for rules in ["mode=cards", "mode=cards tutorial"] {
            let views: Vec<View> = (0..6)
                .map(|_| {
                    let mut app = lesson_game(rules);
                    for _ in 0..30 {
                        crate::arena::test_frame(&mut app, 0.1);
                    }
                    view(&app).unwrap()
                })
                .collect();
            assert!(views.iter().all(|v| *v == views[0]), "{rules}: the same game dealt differently");
            assert!(views[0].events.len() > 1, "{rules}: something was dealt");
        }
    }
    fn step(app: &App) -> Option<Step> {
        app.world().resource::<Tutorial>().lesson.as_ref().map(|lesson| lesson.step)
    }
    /// What the coach box says, a line of text each (None while it is not laid out).
    fn coached(app: &mut App) -> Option<Vec<String>> {
        let world = app.world_mut();
        let (e, node) = world.query_filtered::<(Entity, &Node), With<CoachBox>>().single(world).ok()?;
        if node.display == Display::None {
            return None;
        }
        let children: Vec<Entity> = world.get::<Children>(e).map(|c| c.iter().collect()).unwrap_or_default();
        Some(children.into_iter().filter_map(|c| world.get::<Text>(c).map(|t| t.0.clone())).collect())
    }
    /// Plays on, a tenth of a second a frame, until the table waits on player 1 (its go or its pick, and nothing has
    /// moved for longer than a card shows at the lesson's pace), and returns that view.
    fn until_asked(app: &mut App) -> View {
        let (mut version, mut still) = (u64::MAX, 0.);
        for _ in 0..900 {
            crate::arena::test_frame(app, 0.1);
            let v = view(app).unwrap();
            (still, version) = if v.version == version { (still + 0.1, version) } else { (0., v.version) };
            let (go, picking) = asks(&v, Some(0));
            if (go || picking) && still > kitchen_sim::cards::SHOW * tutorial::PACE + 0.1 {
                return v;
            }
        }
        panic!("the table never asked player 1");
    }
    fn ask(app: &mut App, command: Command) {
        app.world_mut().resource_mut::<Wishes>().0.push(command);
        crate::arena::test_frame(app, 0.01);
    }
    /// How long (seconds) a card dealt in hand `hand` showed before the next was dealt: the time between the first two
    /// cards dealt from now on, at frames of `dt`.
    fn deal_pace(app: &mut App, hand: u32, dt: f32) -> f32 {
        let latest = |v: &View| v.events.iter().filter(|e| e.kind == Kind::Dealt).map(|e| e.seq).max().unwrap_or(0);
        let (mut seen_seq, mut first, mut t) = (latest(&view(app).unwrap()), None, 0.);
        for _ in 0..4000 {
            crate::arena::test_frame(app, dt);
            t += dt;
            let v = view(app).unwrap();
            if v.hand == hand && latest(&v) > seen_seq {
                seen_seq = latest(&v);
                match first {
                    None => first = Some(t),
                    Some(at) => return t - at,
                }
            }
        }
        panic!("hand {hand} was never dealt");
    }

    #[test]
    fn the_tutorial_is_a_guided_first_hand_and_the_game_plays_on_after_it() {
        let mut app = lesson_game("mode=cards tutorial");
        // Laid at once, in place of the game's (nothing dealt yet), and dealt from the next frame: this player first, by
        // the lesson's dealer (the seat before it), slowly.
        assert_eq!(step(&app), Some(Step::Flip));
        assert_eq!(view(&app).unwrap().hand, 0);
        crate::arena::test_frame(&mut app, 0.05);
        let v = view(&app).unwrap();
        assert_eq!((v.dealer, v.hand), (4, 1));
        assert_eq!(v.events.iter().find(|e| e.kind == Kind::Dealt).map(|e| (e.seat, e.card)), Some((0, Some(Card::Topping(8)))));
        let slow = deal_pace(&mut app, 1, 0.05);
        assert!((slow - kitchen_sim::cards::SHOW * tutorial::PACE).abs() < 0.06, "{slow}");
        // Step 1, at its go: the coach box over the prompt line.
        until_asked(&mut app);
        assert_eq!(
            coached(&mut app).unwrap(),
            ["This is Fully Loaded. Each topping is worth its number: you\nstart with Pickles 8, eight points.", "Left-click to flip a card.", "Skip tutorial (K)"]
        );
        // Step 2: the topping flipped, what it is worth and what a bust is.
        ask(&mut app, Command::Flip);
        assert_eq!(step(&app), Some(Step::Topping(Card::Topping(12))));
        assert_eq!(coached(&mut app).unwrap()[0], "Ketchup 12: twelve points. The same topping twice ruins your\nhotdog: a bust.");
        // Step 3: a side; step 4: Back in the Fridge, and the pick.
        until_asked(&mut app);
        ask(&mut app, Command::Flip);
        assert_eq!(step(&app), Some(Step::Side(Card::Side(2))));
        assert_eq!(coached(&mut app).unwrap()[0], "Sides never bust you: Fries add 2.");
        until_asked(&mut app);
        ask(&mut app, Command::Flip);
        assert_eq!(step(&app), Some(Step::Pick(Card::Fridge)));
        let v = until_asked(&mut app);
        assert_eq!(v.choosing, Some((0, Card::Fridge)));
        assert_eq!(coached(&mut app).unwrap()[1], "Look at one and click, or press Esc and click a name.");
        let target = (1..5).find(|s| v.seats[*s].status == Status::In).unwrap();
        ask(&mut app, Command::Target(target));
        assert_eq!(view(&app).unwrap().seats[target].status, Status::Fridged);
        // Step 5: the risk; this player stops.
        assert_eq!(step(&app), Some(Step::Risk));
        until_asked(&mut app);
        assert_eq!(coached(&mut app).unwrap()[..2], ["You have two toppings. Every flip could match one.", "Right-click to stop and bank your points."]);
        ask(&mut app, Command::Stop);
        assert_eq!(step(&app), Some(Step::Out(Status::Stopped)));
        // Step 6: the last word when the hand is over, then the game at a game's pace.
        for _ in 0..600 {
            crate::arena::test_frame(&mut app, 0.1);
            if matches!(step(&app), Some(Step::Wrap { .. })) {
                break;
            }
        }
        assert!(matches!(step(&app), Some(Step::Wrap { .. })));
        assert!(view(&app).unwrap().between);
        assert_eq!(coached(&mut app).unwrap()[0], "First to 100 wins. Seven different toppings is FULLY LOADED:\n+15 and the hand ends.");
        let quick = deal_pace(&mut app, 2, 0.05);
        assert!((quick - kitchen_sim::cards::SHOW).abs() < 0.06, "{quick}");
        assert_eq!(view(&app).unwrap().dealer, 0, "the dealer moves on to seat 0, as in a game");
        for _ in 0..120 {
            crate::arena::test_frame(&mut app, 0.1);
        }
        assert_eq!(step(&app), None, "over once the last word has been up a while");
        assert_eq!(coached(&mut app), None);
        // Given once: the next game is a game.
        app.world_mut().resource_mut::<Game>().begin();
        crate::arena::test_frame(&mut app, 0.05);
        crate::arena::test_frame(&mut app, 0.05);
        assert_eq!(step(&app), None);
        assert_eq!(view(&app).unwrap().dealer, 0);
        assert_eq!(app.world().resource::<Tutorial>().due, Some(false));
    }

    #[test]
    fn the_tutorial_is_skipped_by_k_by_its_note_or_by_touch_stop() {
        // K: over at once, its cards back in the deck, and still this player's go (nothing played).
        let mut app = lesson_game("mode=cards tutorial");
        let before = until_asked(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyK);
        crate::arena::test_frame(&mut app, 0.01);
        assert_eq!(step(&app), None);
        crate::arena::test_frame(&mut app, 0.01);
        let after = view(&app).unwrap();
        assert_eq!(after.deck, 94 - 5, "the lesson's cards are back in the deck");
        assert_eq!(after.turn, Some(0));
        assert!(played(&mut app, &before, Kind::Flipped).is_none() && played(&mut app, &before, Kind::Stopped).is_none());
        assert_eq!(coached(&mut app), None);
        // Then a game: this player's flips are the deck's, at a game's pace.
        ask(&mut app, Command::Flip);
        assert!(played(&mut app, &before, Kind::Flipped).is_some());

        // The note, clicked with the pointer free (where the screen's layout would put it): skipped, nothing captured.
        let mut app = lesson_game("mode=cards tutorial");
        let before = until_asked(&mut app);
        {
            let world = app.world_mut();
            let note = world.query_filtered::<Entity, With<SkipNote>>().single(world).unwrap();
            world.entity_mut(note).insert((ComputedNode { size: Vec2::new(120., 14.), inverse_scale_factor: 1., ..default() }, UiGlobalTransform::from_xy(640., 650.)));
        }
        window(&mut app, |window, cursor| {
            cursor.grab_mode = CursorGrabMode::None;
            window.set_cursor_position(Some(Vec2::new(650., 652.)));
        });
        press(&mut app, MouseButton::Left);
        assert_eq!(step(&app), None);
        assert!(!captured(&mut app), "a click on the note captures nothing");
        assert!(played(&mut app, &before, Kind::Flipped).is_none());

        // By touch, Stop while the lesson asks for a flip skips it rather than stopping.
        let mut app = lesson_game("mode=cards tutorial");
        let before = until_asked(&mut app);
        let mut pad = crate::touch::Pad::default();
        pad.on = true;
        app.world_mut().insert_resource(pad);
        press(&mut app, MouseButton::Right);
        assert_eq!(step(&app), None);
        crate::arena::test_frame(&mut app, 0.01);
        assert!(played(&mut app, &before, Kind::Stopped).is_none(), "a skip, not a stop");
        assert_eq!(view(&app).unwrap().turn, Some(0));
        // (With the mouse the right button is a stop, as ever: the lesson goes on without the steps it can no longer
        // give.)
        let mut app = lesson_game("mode=cards tutorial");
        let before = until_asked(&mut app);
        window(&mut app, |_, cursor| cursor.grab_mode = CursorGrabMode::Locked);
        press(&mut app, MouseButton::Right);
        assert!(played(&mut app, &before, Kind::Stopped).is_some());
        assert_eq!(step(&app), Some(Step::Out(Status::Stopped)));
    }

    #[test]
    fn natively_the_tutorial_is_given_only_when_asked_and_never_in_the_demo_or_online() {
        // Not asked: a game as ever, dealt by seat 0, nothing to skip.
        let mut app = lesson_game("mode=cards");
        assert_eq!(step(&app), None);
        let v = until_asked_or_dealt(&mut app);
        assert_eq!(v.dealer, 0);
        assert_eq!(coached(&mut app), None);
        assert_eq!(app.world().resource::<Tutorial>().due, Some(false), "natively nothing remembers it: not due");
        // The demo watches: no lesson even when asked.
        let mut app = crate::arena::test_app("mode=cards tutorial");
        app.add_plugins(CardsPlugin).add_systems(Update, crate::arena::tick);
        // (A demo from its first frame: the viewer has no seat at the table.)
        app.world_mut().resource_mut::<Game>().demo = true;
        for _ in 0..10 {
            crate::arena::test_frame(&mut app, 0.1);
        }
        assert_eq!(step(&app), None);
        assert_eq!(view(&app).unwrap().dealer, 0);
        // Online: the server's table, as it sent it; no lesson is laid, even when asked.
        use crate::network::tests::{connected, hear, told};
        let (mut app, server, _) = connected();
        app.add_systems(Update, crate::arena::tick);
        app.world_mut().resource_mut::<crate::runtime::Options>().tutorial = true;
        let mut table = kitchen_sim::cards::Table::new(5, 7);
        table.tick(0.);
        let sent = table.view();
        let feast = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::bundled(), false);
        for tick in 1..8 {
            let players = feast.players.iter().map(|p| kitchen_sim::net::Seat { player: p.clone(), human: p.id == 2, ..default() }).collect();
            let frame = kitchen_sim::net::Frame { kind: "snapshot".into(), tick, round: 1, rules: "mode=cards".into(), stage: "Warmup".into(), players, ..default() };
            let mut said = vec![crate::network::Incoming::Text(serde_json::to_string(&frame).unwrap())];
            if tick == 1 {
                said.insert(0, told(&sent));
            }
            app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_millis(50));
            hear(&mut app, &server, said);
        }
        assert!(at_table(app.world().resource::<Game>()));
        assert_eq!(app.world().resource::<Seen>().me, Some(1));
        assert_eq!(view(&app).as_ref(), Some(&sent), "the server's table, untouched");
        assert_eq!(step(&app), None);
        assert_eq!(coached(&mut app), None);
    }
    /// Plays on until the first hand has been dealt round (or the table asks player 1), and returns that view.
    fn until_asked_or_dealt(app: &mut App) -> View {
        for _ in 0..200 {
            crate::arena::test_frame(app, 0.1);
            let v = view(app).unwrap();
            if v.events.iter().filter(|e| e.kind == Kind::Dealt).count() >= 5 || asks(&v, Some(0)) != (false, false) {
                return v;
            }
        }
        panic!("the first hand was never dealt");
    }

    #[test]
    fn the_coach_box_breaks_its_lines_at_spaces() {
        assert_eq!(wrap("one two three", 7), "one two\nthree");
        assert_eq!(wrap("a  b", 10), "a b");
        assert_eq!(wrap("unbreakable", 4), "unbreakable");
        assert_eq!(wrap("", 4), "");
        let long = "First to 100 wins. Seven different toppings is FULLY LOADED: +15 and the hand ends.";
        assert!(wrap(long, COACH_LINE).lines().all(|line| line.len() <= COACH_LINE));
        assert_eq!(wrap(long, COACH_LINE).replace('\n', " "), long);
        // As long a line as the window has room for: the most on a wide one, fewer on a phone held upright, never
        // fewer than COACH_SHORT.
        assert_eq!(coach_line(1280.), COACH_LINE);
        assert_eq!(coach_line(480.), 46);
        assert!(coach_line(480.) as f32 * CHAR + 60. <= 480.);
        assert_eq!(coach_line(200.), COACH_SHORT);
        assert_eq!(coach_line(f32::NAN), COACH_SHORT);
    }
}

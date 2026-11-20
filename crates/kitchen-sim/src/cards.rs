// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Fully Loaded: the rules of the hotdogs' card table. A press-your-luck game of toppings, played to TARGET points
//! with a deck of 94 cards; this module is only the rules (no map, no kitchen, no Bevy), so the server and the
//! single-player page can both run one and a test can replay one card by card.
//!
//! **The cards.** The 79 toppings are worth their number (Plain Bun 0 up to Ketchup 12) and there are as many copies of
//! a topping as its number (one Plain Bun), so the best toppings are the commonest and the likeliest to ruin you. Five
//! sides (Fries +2 to Onion Rings +10) and a Foot-Long (doubles the hand's toppings) never hurt. Three each of Back
//! in the Fridge, Pile It On and Napkin.
//!
//! **A hand.** Everyone is dealt one card face up, round from the seat after the dealer; then, in turn, a hotdog
//! flips one more card or stops and banks. A topping you already hold busts you (the hand is worth nothing) unless
//! you hold a Napkin, which wipes the duplicate away (both are discarded). Seven different toppings is fully loaded:
//! +BONUS, and the hand ends for everyone at once. Stopped, busted and fridged hotdogs are skipped; the hand ends when
//! none is left in. The totals are banked then, and the first to TARGET wins (a tie at the top plays another hand
//! between the tied). The dealer moves one seat each hand.
//!
//! **Action cards** are played the moment they are drawn, by whoever drew them, on a hotdog still in the hand (their
//! own included): Back in the Fridge stops it and banks its hand; Pile It On makes it flip PILE cards, one at a time,
//! stopping early on a bust or when fully loaded, and action cards it flips meanwhile wait until the three are done (or
//! are discarded if it is no longer in). A Napkin is kept at once; a second goes to the next hotdog still in without one,
//! else it is discarded.
//!
//! **The clock.** A `Table` keeps its own time (`tick`): a card shows for SHOW seconds before play moves on, a bot
//! thinks for THINK seconds, a person has a decision time (`set_limit`; none by default) after which they stop (or, if
//! they were picking a target, the bots' choice is made for them), the totals show for BETWEEN seconds before the next
//! hand, and once somebody has won the table rests (`winner`). A person who leaves is replaced with `set_bot`, which
//! takes over a decision in progress after a think of a second or so rather than the rest of the clock.
//!
//! **Driving it.** The server calls `tick` with the elapsed time, `command` with what each client sent (any seat
//! number, any command: it answers `Err` and changes nothing rather than trust it), and sends `view` to the clients
//! whenever `version` has moved. Nothing is dealt until the first `tick`.
//!
//! **A lesson.** The single-player tutorial plays its first hand from a table laid by `with_order`: the hand is dealt
//! by the seat it names, and each seat draws the cards it was given, in order, before any from the shuffled deck, so
//! the lesson's cards come up whatever the bots decide. Its cards come out of the deck (none is ever twice), and those
//! nobody drew go back into it, shuffled, when the hand ends (or at `clear_order`). `set_pace` slows the table's shows
//! and its bots' thinking while the lesson is read. A game never calls either: `new` has no order and its pace is 1.
use serde::{Deserialize, Serialize};
use std::{cmp::Reverse, collections::VecDeque};

/// The total that wins the game.
pub const TARGET: u32 = 100;
/// What fully loaded adds to the hand.
pub const BONUS: u32 = 15;
/// Different toppings that make a hotdog fully loaded.
pub const LOADED: usize = 7;
/// Cards a Pile It On makes a hotdog flip.
pub const PILE: usize = 3;
/// Seconds a card shows before play moves on, a person's decision (online), and the scores between hands.
pub const SHOW: f32 = 0.7;
pub const TURN: f32 = 15.;
pub const BETWEEN: f32 = 3.;
/// The seconds a bot thinks for, least and most.
pub const THINK: [f32; 2] = [0.8, 1.6];
/// The toppings; the index is the value (and the number of copies, but for the Plain Bun's one).
pub const TOPPINGS: [&str; 13] = [
    "Plain Bun",
    "Celery Salt",
    "Sport Peppers",
    "Jalapeños",
    "Bacon",
    "Cheese",
    "Chili",
    "Sauerkraut",
    "Pickles",
    "Onions",
    "Relish",
    "Mustard",
    "Ketchup",
];
/// The sides by what they are worth.
const SIDES: [(u8, &str); 5] = [(2, "Fries"), (4, "Chips"), (6, "Pickle Spear"), (8, "Soda"), (10, "Onion Rings")];
/// How many events a view carries.
const EVENTS: usize = 16;
/// The longest a single `tick` runs for.
const LONGEST: f32 = 60.;
/// What a bot will risk: the chance of busting it accepts on a flip, raised by a Napkin and lowered by a hand worth
/// RICH points; and when it fridges itself rather than another.
const NERVE: f32 = 0.30;
const PADDED: f32 = 0.25;
const RICH: u32 = 30;
const GREED: f32 = 0.10;
const DANGER: f32 = 0.4;
const WORTH_SAVING: u32 = 20;

/// A card. A topping carries its value (the index into TOPPINGS), a side what it is worth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Card {
    Topping(u8),
    Side(u8),
    FootLong,
    /// Back in the Fridge.
    Fridge,
    PileOn,
    Napkin,
}

impl Card {
    /// What the table prints on it: "Ketchup 12", "Fries +2", "Foot-Long ×2", "Back in the Fridge", "Pile It On",
    /// "Napkin".
    pub fn name(self) -> String {
        match self {
            Card::Topping(value) => format!("{} {value}", TOPPINGS.get(value as usize).copied().unwrap_or("Topping")),
            Card::Side(value) => format!("{} +{value}", SIDES.iter().find(|(v, _)| *v == value).map_or("Side", |(_, name)| name)),
            Card::FootLong => "Foot-Long ×2".into(),
            Card::Fridge => "Back in the Fridge".into(),
            Card::PileOn => "Pile It On".into(),
            Card::Napkin => "Napkin".into(),
        }
    }
    /// Played the moment it is drawn, on a hotdog its drawer picks.
    pub fn is_action(self) -> bool {
        matches!(self, Card::Fridge | Card::PileOn)
    }
}

/// The 94 cards, in order and unshuffled.
pub fn deck() -> Vec<Card> {
    let mut cards = Vec::with_capacity(94);
    for value in 0..TOPPINGS.len() as u8 {
        cards.extend((0..value.max(1)).map(|_| Card::Topping(value)));
    }
    cards.extend(SIDES.iter().map(|(value, _)| Card::Side(*value)));
    cards.push(Card::FootLong);
    for card in [Card::Fridge, Card::PileOn, Card::Napkin] {
        cards.extend([card; 3]);
    }
    cards
}

/// Where a hotdog stands in the hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    In,
    Stopped,
    Fridged,
    Bust,
    Loaded,
    /// Not in this hand (a tie-break is played by the tied only), or no hand has begun.
    Out,
}

/// What a client may ask of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Flip,
    Stop,
    /// The hotdog an action card is played on.
    Target(usize),
}

/// What happened, for the clients to show and sound. `seat` is whose it was (who flipped, was dealt, played the
/// action card, or won; for `Hand`, the dealer), `card` the card concerned and `target` the other hotdog (for
/// `Fridged`, `PiledOn` and a `Passed` Napkin; None if it was discarded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Dealt,
    Flipped,
    /// A duplicate topping, `card`, ruined the hand.
    Bust,
    /// A Napkin wiped away the duplicate `card`.
    Saved,
    /// The seventh different topping, `card`.
    Loaded,
    Stopped,
    Fridged,
    PiledOn,
    Passed,
    /// The hand is over and the totals are banked.
    Hand,
    Won,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Counts from 1 over the whole game, so a client can tell which events it has seen.
    pub seq: u64,
    pub seat: usize,
    pub kind: Kind,
    pub card: Option<Card>,
    pub target: Option<usize>,
}

/// One seat as the clients see it: its cards face up (a Napkin held, a bust's duplicate and all), this hand's points
/// (0 for a bust), its total banked in earlier hands (this hand's too once the hand is over) and whether a bot plays it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeatView {
    pub cards: Vec<Card>,
    pub status: Status,
    pub points: u32,
    pub total: u32,
    pub napkin: bool,
    pub bot: bool,
}

/// Everything the clients show. `turn` is whose go it is, from their turn coming up until the next one's (None while
/// dealing, between hands and once it is over); `choosing` who is picking a hotdog to play which action card; `clock`
/// the seconds left of a person's decision (None for a bot's and when there is no limit); `deck` the cards left to draw;
/// `between` that the scores are showing before the next hand; `events` the latest 16.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub seats: Vec<SeatView>,
    pub dealer: usize,
    pub hand: u32,
    pub turn: Option<usize>,
    pub choosing: Option<(usize, Card)>,
    pub clock: Option<f32>,
    pub deck: usize,
    pub between: bool,
    pub winner: Option<usize>,
    pub events: Vec<Event>,
    pub version: u64,
}

#[derive(Clone, Debug)]
struct Seat {
    cards: Vec<Card>,
    status: Status,
    total: u32,
    bot: bool,
}

impl Seat {
    /// This hand's points as it stands: nothing for a bust, a seat that is out or (before they are dealt) no cards.
    fn points(&self) -> u32 {
        match self.status {
            Status::Bust | Status::Out => 0,
            status => Table::points(&self.cards, status == Status::Loaded),
        }
    }
    fn napkin(&self) -> bool {
        self.cards.contains(&Card::Napkin)
    }
    fn toppings(&self) -> usize {
        self.cards.iter().filter(|c| matches!(c, Card::Topping(_))).count()
    }
}

/// What is left to do, front first.
#[derive(Clone, Copy, Debug)]
enum Job {
    /// The seat is dealt a card (skipped if it is not in).
    Deal(usize),
    /// The seat flips a card (skipped if it is no longer in: how a Pile It On stops at a bust).
    Draw(usize),
    /// The seat plays an action card it drew (discarded if it is no longer in).
    Action(usize, Card),
    /// The seat's go.
    Turn(usize),
}

/// What the table is waiting for.
#[derive(Clone, Copy, Debug)]
enum Wait {
    /// The first tick deals.
    Start,
    /// Nothing: the jobs are to be run.
    Idle,
    /// A card is on show for so many seconds.
    Show(f32),
    /// A seat is to flip or stop (`pick` None) or to pick a hotdog to play the card on. `timer` is a bot's think or a
    /// person's time left (None: no limit).
    Ask { seat: usize, pick: Option<Card>, timer: Option<f32> },
    /// The scores are on show for so many seconds.
    Between(f32),
    Over,
}

/// The card table. See the module's notes.
#[derive(Clone, Debug)]
pub struct Table {
    /// Drawn from the end.
    deck: Vec<Card>,
    discards: Vec<Card>,
    seats: Vec<Seat>,
    /// Who sits in the next hand (all of them, or the tied at the top).
    playing: Vec<bool>,
    dealer: usize,
    hand: u32,
    jobs: VecDeque<Job>,
    wait: Wait,
    /// The last seat to have had a go.
    cursor: usize,
    turn: Option<usize>,
    events: Vec<Event>,
    seq: u64,
    version: u64,
    rng: u64,
    limit: Option<f32>,
    winner: Option<usize>,
    /// A lesson's cards (`with_order`): what each seat draws next, front first, before the deck. Empty in a game.
    order: Vec<VecDeque<Card>>,
    /// How many times longer than SHOW, THINK and BETWEEN the table waits (`set_pace`): 1 in a game.
    pace: f32,
}

impl Table {
    /// A table of `seats` (at least one) with a shuffled deck. Seat 0 deals the first hand; nothing is dealt until the
    /// first `tick`. Every seat is a person, with no time limit, until `set_bot` and `set_limit` say otherwise.
    pub fn new(seats: usize, seed: u64) -> Table {
        let seats = seats.max(1);
        // A small seed on its own would start xorshift in a rut (and `| 1` makes 2 and 3 the same), so mix it first.
        let mut mixed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^= mixed >> 31;
        let mut table = Table {
            deck: deck(),
            discards: Vec::new(),
            seats: (0..seats).map(|_| Seat { cards: Vec::new(), status: Status::Out, total: 0, bot: false }).collect(),
            playing: vec![true; seats],
            dealer: 0,
            hand: 0,
            jobs: VecDeque::new(),
            wait: Wait::Start,
            cursor: 0,
            turn: None,
            events: Vec::new(),
            seq: 0,
            version: 0,
            rng: mixed | 1,
            limit: None,
            winner: None,
            order: Vec::new(),
            pace: 1.,
        };
        table.shuffle();
        table
    }

    /// A table for a lesson (the tutorial): as `new`, but the first hand is dealt by `dealer` (so the seat after it is
    /// dealt first and has the first go), and seat `s` draws the cards of `order[s]` in that hand, front first, before
    /// any from the deck: the card it is dealt, then its flips (a Pile It On's included). Each card is taken out of the
    /// shuffled deck, so no card is ever twice; one the deck has no copy of left is left out. The cards a seat does not
    /// draw go back into the deck, shuffled, when the hand ends. Every later hand is a game's.
    pub fn with_order(seats: usize, seed: u64, dealer: usize, order: Vec<Vec<Card>>) -> Table {
        let mut table = Table::new(seats, seed);
        table.dealer = dealer % table.seats.len();
        for cards in order.into_iter().take(table.seats.len()) {
            let mut set = VecDeque::new();
            for card in cards {
                if let Some(at) = table.deck.iter().position(|c| *c == card) {
                    set.push_back(table.deck.remove(at));
                }
            }
            table.order.push(set);
        }
        table
    }

    /// Ends a lesson's order now (`with_order`): the cards still set for the seats go back into the deck, shuffled,
    /// and every draw from now on is from the deck. (A table without an order is left as it is.)
    pub fn clear_order(&mut self) {
        let left: Vec<Card> = self.order.drain(..).flatten().collect();
        if !left.is_empty() {
            self.deck.extend(left);
            self.shuffle();
            self.version += 1;
        }
    }

    /// How slowly the table plays: `pace` times as long for a card to show, a bot to think and the scores between hands
    /// (a person's time limit is as it was). 1 is a game's pace; the tutorial's is slower, so that a new player can
    /// follow the bots. Anything but a positive number is 1. A wait under way keeps the time it had.
    pub fn set_pace(&mut self, pace: f32) {
        self.pace = if pace.is_finite() && pace > 0. { pace } else { 1. };
    }

    /// A hand's points: the toppings' values added up (doubled by a Foot-Long), plus the sides, plus BONUS if fully
    /// loaded. (Whether a hand busted is for the caller: a bust is worth nothing.)
    pub fn points(cards: &[Card], loaded: bool) -> u32 {
        let (mut toppings, mut sides, mut double) = (0, 0, false);
        for card in cards {
            match *card {
                Card::Topping(value) => toppings += value as u32,
                Card::Side(value) => sides += value as u32,
                Card::FootLong => double = true,
                _ => {}
            }
        }
        toppings * if double { 2 } else { 1 } + sides + if loaded { BONUS } else { 0 }
    }

    /// Whether a bot plays the seat. Taking over a decision in progress, a bot answers after its think time; a person
    /// taking one back has the whole of the limit. (A seat that does not exist is ignored.)
    pub fn set_bot(&mut self, seat: usize, bot: bool) {
        let Some(s) = self.seats.get_mut(seat) else { return };
        if s.bot == bot {
            return;
        }
        s.bot = bot;
        self.version += 1;
        if matches!(self.wait, Wait::Ask { seat: asked, .. } if asked == seat) {
            let timer = if bot { Some(self.think()) } else { self.limit };
            if let Wait::Ask { timer: slot, .. } = &mut self.wait {
                *slot = timer;
            }
        }
    }

    /// A person's time for a decision (None: as long as they like, the default); a decision under way starts again.
    pub fn set_limit(&mut self, limit: Option<f32>) {
        self.limit = limit.filter(|l| l.is_finite() && *l > 0.);
        match &mut self.wait {
            Wait::Ask { seat, timer, .. } if !self.seats[*seat].bot => *timer = self.limit,
            _ => {}
        }
    }

    /// Sets a seat's banked total. For tests (this crate's and the server's), to put a game near its end; a seat that
    /// does not exist is ignored.
    pub fn set_total(&mut self, seat: usize, total: u32) {
        if let Some(s) = self.seats.get_mut(seat) {
            s.total = total;
            self.version += 1;
        }
    }

    /// Who has won, once somebody has.
    pub fn winner(&self) -> Option<usize> {
        self.winner
    }

    /// Raised by every change of what `view` shows but the clock, so a server can send only when this moves.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn view(&self) -> View {
        let (choosing, clock) = match self.wait {
            Wait::Ask { seat, pick, timer } => {
                (pick.map(|card| (seat, card)), timer.filter(|_| !self.seats[seat].bot).map(|t| t.max(0.)))
            }
            _ => (None, None),
        };
        let seat = |s: &Seat| SeatView {
            cards: s.cards.clone(),
            status: s.status,
            points: s.points(),
            total: s.total,
            napkin: s.napkin(),
            bot: s.bot,
        };
        View {
            seats: self.seats.iter().map(seat).collect(),
            dealer: self.dealer,
            hand: self.hand,
            turn: self.turn,
            choosing,
            clock,
            deck: self.deck.len(),
            between: matches!(self.wait, Wait::Between(_)),
            winner: self.winner,
            events: self.events.clone(),
            version: self.version,
        }
    }

    /// Lets `dt` seconds pass: shows run out, bots make up their minds, a person's time runs out. Any `dt` will do
    /// (a long one plays on through everything that falls inside it, in order; a negative or NaN one is nothing), but
    /// at most a minute is played at once.
    pub fn tick(&mut self, dt: f32) {
        // (Not `clamp`, which lets a NaN through.)
        #[allow(clippy::manual_clamp)]
        let mut left = dt.max(0.).min(LONGEST);
        if matches!(self.wait, Wait::Start) {
            self.begin_hand();
        }
        loop {
            let Some(timer) = self.timer() else { return };
            if *timer > left {
                *timer -= left;
                return;
            }
            left -= *timer;
            self.expire();
        }
    }

    /// Plays a seat's card: `Flip` or `Stop` on its turn, `Target` when it is picking a hotdog to play an action card
    /// on (one still in the hand, itself included). Anything else, from any seat number, is refused and changes nothing.
    /// (The table does not know who owns a seat: that is for the server.)
    pub fn command(&mut self, seat: usize, command: Command) -> Result<(), &'static str> {
        if seat >= self.seats.len() {
            return Err("no such seat");
        }
        let Wait::Ask { seat: asked, pick, .. } = self.wait else { return Err("nothing to decide just now") };
        if asked != seat {
            return Err("not your turn");
        }
        match (command, pick) {
            (Command::Flip | Command::Stop, None) => {}
            (Command::Target(at), Some(_)) => {
                if !self.seats.get(at).is_some_and(|s| s.status == Status::In) {
                    return Err("pick a hotdog still in the hand");
                }
            }
            (Command::Target(_), None) => return Err("nothing to pick"),
            (Command::Flip | Command::Stop, Some(_)) => return Err("pick a hotdog first"),
        }
        self.answer(command);
        Ok(())
    }

    /// Puts these cards on top of the deck, the first to be drawn first (a copy of each taken from the deck, or the
    /// discards, so no card is ever twice). For tests.
    #[cfg(test)]
    fn stack(&mut self, top: Vec<Card>) {
        for card in &top {
            if let Some(at) = self.deck.iter().position(|c| c == card) {
                self.deck.remove(at);
            } else if let Some(at) = self.discards.iter().position(|c| c == card) {
                self.discards.remove(at);
            }
        }
        self.deck.extend(top.into_iter().rev());
    }

    // ---- the table's own random numbers ----

    /// xorshift64*.
    fn roll(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// A number from 0 up to but not including 1.
    fn unit(&mut self) -> f32 {
        (self.roll() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn think(&mut self) -> f32 {
        (THINK[0] + self.unit() * (THINK[1] - THINK[0])) * self.pace
    }
    fn shuffle(&mut self) {
        for i in (1..self.deck.len()).rev() {
            let j = (self.roll() % (i as u64 + 1)) as usize;
            self.deck.swap(i, j);
        }
    }

    // ---- the clock ----

    /// The seconds counting down, if something is.
    fn timer(&mut self) -> Option<&mut f32> {
        match &mut self.wait {
            Wait::Show(t) | Wait::Between(t) => Some(t),
            Wait::Ask { timer, .. } => timer.as_mut(),
            Wait::Start | Wait::Idle | Wait::Over => None,
        }
    }

    /// The countdown has run out.
    fn expire(&mut self) {
        match self.wait {
            Wait::Show(_) => {
                self.wait = Wait::Idle;
                self.run();
            }
            Wait::Between(_) => self.begin_hand(),
            // A bot has thought, or a person is out of time: a person stops, and the bots' choice is made of a pick.
            Wait::Ask { seat, pick, .. } => {
                let command = match pick {
                    Some(card) => Command::Target(self.choose(seat, card)),
                    None if self.seats[seat].bot && self.wants_flip(seat) => Command::Flip,
                    None => Command::Stop,
                };
                self.answer(command);
            }
            Wait::Start | Wait::Idle | Wait::Over => {}
        }
    }

    // ---- the hand ----

    fn event(&mut self, kind: Kind, seat: usize, card: Option<Card>, target: Option<usize>) {
        self.seq += 1;
        self.events.push(Event { seq: self.seq, seat, kind, card, target });
        if self.events.len() > EVENTS {
            self.events.remove(0);
        }
        self.version += 1;
    }

    fn begin_hand(&mut self) {
        let n = self.seats.len();
        for seat in &mut self.seats {
            self.discards.append(&mut seat.cards);
        }
        // (The first hand's dealer is seat 0's in a game, a lesson's own in a lesson: `with_order`.)
        self.dealer = if self.hand == 0 { self.dealer } else { (self.dealer + 1) % n };
        self.hand += 1;
        for (seat, playing) in self.seats.iter_mut().zip(&self.playing) {
            seat.status = if *playing { Status::In } else { Status::Out };
        }
        self.turn = None;
        self.cursor = self.dealer;
        self.jobs.clear();
        for k in 1..=n {
            let seat = (self.dealer + k) % n;
            if self.playing[seat] {
                self.jobs.push_back(Job::Deal(seat));
            }
        }
        self.version += 1;
        self.wait = Wait::Idle;
        self.run();
    }

    /// Does the jobs until something has to be waited for.
    fn run(&mut self) {
        while matches!(self.wait, Wait::Idle) {
            match self.jobs.pop_front() {
                Some(job) => self.job(job),
                None => self.settle(),
            }
        }
    }

    fn job(&mut self, job: Job) {
        let is_in = |t: &Table, seat: usize| t.seats[seat].status == Status::In;
        match job {
            Job::Deal(seat) if is_in(self, seat) => self.draw(seat, Kind::Dealt),
            Job::Draw(seat) if is_in(self, seat) => self.draw(seat, Kind::Flipped),
            Job::Action(seat, card) => self.action(seat, card),
            Job::Turn(seat) if is_in(self, seat) => {
                self.turn = Some(seat);
                self.cursor = seat;
                self.open(seat, None);
            }
            Job::Deal(_) | Job::Draw(_) | Job::Turn(_) => {}
        }
    }

    /// Nothing is left to do: the next go, or the end of the hand.
    fn settle(&mut self) {
        let n = self.seats.len();
        let over = self.seats.iter().any(|s| s.status == Status::Loaded);
        let next = (1..=n).map(|k| (self.cursor + k) % n).find(|&s| self.seats[s].status == Status::In);
        match next {
            Some(seat) if !over => self.jobs.push_back(Job::Turn(seat)),
            _ => self.end_hand(),
        }
    }

    /// Waits for a seat: a bot for a moment, a person for as long as the limit gives.
    fn open(&mut self, seat: usize, pick: Option<Card>) {
        let timer = if self.seats[seat].bot { Some(self.think()) } else { self.limit };
        self.wait = Wait::Ask { seat, pick, timer };
        self.version += 1;
    }

    /// The seat's answer to what it was asked.
    fn answer(&mut self, command: Command) {
        let Wait::Ask { seat, pick, .. } = self.wait else { return };
        self.wait = Wait::Idle;
        self.version += 1;
        match (command, pick) {
            (Command::Flip, _) => self.jobs.push_front(Job::Draw(seat)),
            (Command::Stop, _) => self.stop(seat),
            (Command::Target(at), Some(card)) => self.resolve(seat, card, at),
            (Command::Target(_), None) => {}
        }
        self.run();
    }

    /// The next card for `seat`: a lesson's card set for it if there is one (`with_order`), else the deck's, the discards
    /// shuffled into a new deck if there is none left; None if there is none anywhere.
    fn take(&mut self, seat: usize) -> Option<Card> {
        if let Some(card) = self.order.get_mut(seat).and_then(VecDeque::pop_front) {
            return Some(card);
        }
        if self.deck.is_empty() {
            std::mem::swap(&mut self.deck, &mut self.discards);
            self.shuffle();
        }
        self.deck.pop()
    }

    /// The seat is dealt or flips a card.
    fn draw(&mut self, seat: usize, kind: Kind) {
        let Some(card) = self.take(seat) else {
            // Not a card left in the world (all of them in front of the seats): a flip can only be given up.
            if kind == Kind::Flipped {
                self.stop(seat);
            }
            return;
        };
        self.event(kind, seat, Some(card), None);
        self.wait = Wait::Show(SHOW * self.pace);
        match card {
            Card::Topping(_) => self.topping(seat, card),
            Card::Side(_) | Card::FootLong => self.seats[seat].cards.push(card),
            Card::Napkin => self.napkin(seat),
            // After the cards its drawer is already to flip, if any, and the actions it flipped before.
            Card::Fridge | Card::PileOn => {
                let waiting = self.jobs.iter().take_while(|j| matches!(j, Job::Draw(s) | Job::Action(s, _) if *s == seat)).count();
                self.jobs.insert(waiting, Job::Action(seat, card));
            }
        }
    }

    fn topping(&mut self, seat: usize, card: Card) {
        let cards = &mut self.seats[seat].cards;
        if !cards.contains(&card) {
            cards.push(card);
            if self.seats[seat].toppings() >= LOADED {
                // Fully loaded ends the hand at once, for everyone: nothing more is dealt, flipped or played, so the
                // jobs are dropped (a Pile It On's remaining flips, and any seat's action cards still waiting, which are
                // discarded) and the hand ends when the card has been shown. No pick is open: this is a flip.
                self.seats[seat].status = Status::Loaded;
                for job in std::mem::take(&mut self.jobs) {
                    if let Job::Action(_, card) = job {
                        self.discards.push(card);
                    }
                }
                self.event(Kind::Loaded, seat, Some(card), None);
            }
        } else if let Some(at) = cards.iter().position(|c| *c == Card::Napkin) {
            cards.remove(at);
            self.discards.extend([Card::Napkin, card]);
            self.event(Kind::Saved, seat, Some(card), None);
        } else {
            cards.push(card);
            self.seats[seat].status = Status::Bust;
            self.event(Kind::Bust, seat, Some(card), None);
        }
    }

    /// A Napkin is kept; a second goes to the next hotdog still in that has none, else it is discarded.
    fn napkin(&mut self, seat: usize) {
        let n = self.seats.len();
        if !self.seats[seat].napkin() {
            self.seats[seat].cards.push(Card::Napkin);
            return;
        }
        let to = (1..n).map(|k| (seat + k) % n).find(|&s| self.seats[s].status == Status::In && !self.seats[s].napkin());
        match to {
            Some(to) => self.seats[to].cards.push(Card::Napkin),
            None => self.discards.push(Card::Napkin),
        }
        self.event(Kind::Passed, seat, Some(Card::Napkin), to);
    }

    fn stop(&mut self, seat: usize) {
        self.seats[seat].status = Status::Stopped;
        self.event(Kind::Stopped, seat, None, None);
    }

    /// An action card is drawn (or comes up again after a Pile It On): its drawer picks a hotdog, or is its own target.
    fn action(&mut self, seat: usize, card: Card) {
        if self.seats[seat].status != Status::In {
            self.discards.push(card);
        } else if self.seats.iter().enumerate().any(|(s, other)| s != seat && other.status == Status::In) {
            self.open(seat, Some(card));
        } else {
            self.resolve(seat, card, seat);
        }
    }

    /// `seat` plays the action card on `at`, a hotdog still in.
    fn resolve(&mut self, seat: usize, card: Card, at: usize) {
        self.discards.push(card);
        match card {
            Card::Fridge => {
                self.seats[at].status = Status::Fridged;
                self.event(Kind::Fridged, seat, Some(card), Some(at));
            }
            Card::PileOn => {
                self.event(Kind::PiledOn, seat, Some(card), Some(at));
                for _ in 0..PILE {
                    self.jobs.push_front(Job::Draw(at));
                }
            }
            _ => {}
        }
    }

    /// The hand is over: what is in banks (and what is not is as it was), and the game too perhaps.
    fn end_hand(&mut self) {
        self.turn = None;
        self.clear_order();
        for seat in &mut self.seats {
            if seat.status == Status::In {
                seat.status = Status::Stopped;
            }
            seat.total += seat.points();
        }
        self.event(Kind::Hand, self.dealer, None, None);
        let top = self.seats.iter().map(|s| s.total).max().unwrap_or(0);
        if top >= TARGET {
            let leaders: Vec<usize> = (0..self.seats.len()).filter(|&s| self.seats[s].total == top).collect();
            if let [winner] = leaders[..] {
                self.winner = Some(winner);
                self.wait = Wait::Over;
                self.event(Kind::Won, winner, None, None);
                return;
            }
            // A tie at the top: one more hand between the tied.
            for (seat, playing) in self.playing.iter_mut().enumerate() {
                *playing = leaders.contains(&seat);
            }
        } else {
            self.playing.fill(true);
        }
        self.wait = Wait::Between(BETWEEN * self.pace);
        self.version += 1;
    }

    // ---- the bots ----

    /// The cards a flip comes from (the discards, which are about to be shuffled in, if the deck is out).
    fn pool(&self) -> &[Card] {
        if self.deck.is_empty() { &self.discards } else { &self.deck }
    }

    fn wants_flip(&self, seat: usize) -> bool {
        let s = &self.seats[seat];
        wants_flip(&s.cards, s.total, self.pool())
    }

    /// The hotdog a bot plays an action card on (also what is done for a person who is out of time). Back in the Fridge
    /// goes to the opponent in with the most at stake (hand and total), but to the bot itself if it is likely to bust
    /// and has a hand worth keeping, or if it is alone. Pile It On goes to the opponent in with the most toppings (the
    /// likeliest to bust; the higher total of two), else itself. Where it is a tie the lower seat is picked.
    fn choose(&self, seat: usize, card: Card) -> usize {
        let me = &self.seats[seat];
        let others = (0..self.seats.len()).filter(|&s| s != seat && self.seats[s].status == Status::In);
        match card {
            Card::Fridge if risk(&me.cards, self.pool()) > DANGER && me.points() >= WORTH_SAVING => seat,
            Card::Fridge => others.max_by_key(|&s| (self.seats[s].total + self.seats[s].points(), Reverse(s))).unwrap_or(seat),
            _ => others.max_by_key(|&s| (self.seats[s].toppings(), self.seats[s].total, Reverse(s))).unwrap_or(seat),
        }
    }
}

/// The chance that the next card from `pool` is a topping the hand already holds.
fn risk(cards: &[Card], pool: &[Card]) -> f32 {
    if pool.is_empty() {
        return 0.;
    }
    let hits = pool.iter().filter(|c| matches!(c, Card::Topping(_)) && cards.contains(c)).count();
    hits as f32 / pool.len() as f32
}

/// Whether a bot with this hand and total flips: not if the hand alone wins the game; else if the chance of a bust is
/// no more than what it will risk, which a Napkin raises and a good hand lowers.
fn wants_flip(cards: &[Card], total: u32, pool: &[Card]) -> bool {
    let points = Table::points(cards, false);
    if points > 0 && points + total >= TARGET {
        return false;
    }
    let mut limit = NERVE;
    if cards.contains(&Card::Napkin) {
        limit += PADDED;
    }
    if points >= RICH {
        limit -= GREED;
    }
    risk(cards, pool) <= limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use Card::{FootLong, Fridge, Napkin, PileOn, Side, Topping};
    use std::collections::HashMap;

    /// A table of persons (no clock), seeded so that a test replays exactly.
    fn table(seats: usize) -> Table {
        Table::new(seats, 7)
    }
    /// Runs the table until somebody has to act, the scores are showing between hands, or the game is over.
    fn settle(t: &mut Table) {
        for _ in 0..4000 {
            if matches!(t.wait, Wait::Ask { .. } | Wait::Between(_) | Wait::Over) {
                return;
            }
            t.tick(0.1);
        }
        panic!("the table never settled");
    }
    /// Seats 1, 2, 3, 4 then 0 are dealt Ketchup, Mustard, Relish, Onions and Pickles: harmless, and all different.
    const DEAL: [Card; 5] = [Topping(12), Topping(11), Topping(10), Topping(9), Topping(8)];
    fn deal(t: &mut Table, cards: &[Card]) {
        t.stack(cards.to_vec());
        settle(t);
    }
    fn flip(t: &mut Table, seat: usize, card: Card) {
        t.stack(vec![card]);
        t.command(seat, Command::Flip).unwrap();
        settle(t);
    }
    fn stop(t: &mut Table, seat: usize) {
        t.command(seat, Command::Stop).unwrap();
        settle(t);
    }
    fn target(t: &mut Table, seat: usize, at: usize) {
        t.command(seat, Command::Target(at)).unwrap();
        settle(t);
    }
    /// Puts exactly these cards in front of a seat (what lay there goes to the discards, these come off the deck), so
    /// that every card is still somewhere once.
    fn hold(t: &mut Table, seat: usize, cards: &[Card]) {
        t.stack(cards.to_vec());
        let taken = t.deck.split_off(t.deck.len() - cards.len());
        let before = std::mem::replace(&mut t.seats[seat].cards, taken);
        t.discards.extend(before);
    }
    fn events(t: &Table, kind: Kind) -> Vec<Event> {
        t.view().events.into_iter().filter(|e| e.kind == kind).collect()
    }
    /// Every card the table knows of, wherever it lies: in the deck, the discards, in front of a seat, set for a seat
    /// by a lesson, waiting in a job or being picked for.
    fn census(t: &Table) -> HashMap<Card, usize> {
        let mut all = HashMap::new();
        let mut add = |card: &Card| *all.entry(*card).or_insert(0) += 1;
        t.deck.iter().for_each(&mut add);
        t.order.iter().flatten().for_each(&mut add);
        t.discards.iter().for_each(&mut add);
        t.seats.iter().flat_map(|s| &s.cards).for_each(&mut add);
        for job in &t.jobs {
            if let Job::Action(_, card) = job {
                add(card);
            }
        }
        if let Wait::Ask { pick: Some(card), .. } = &t.wait {
            add(card);
        }
        all
    }
    fn whole_deck() -> HashMap<Card, usize> {
        let mut all = HashMap::new();
        for card in deck() {
            *all.entry(card).or_insert(0) += 1;
        }
        all
    }

    #[test]
    fn the_deck_is_94_cards_with_as_many_copies_of_a_topping_as_its_value() {
        let d = deck();
        let copies = |card: Card| d.iter().filter(|c| **c == card).count();
        assert_eq!(d.len(), 94);
        assert_eq!(d.iter().filter(|c| matches!(c, Topping(_))).count(), 79);
        assert_eq!(copies(Topping(0)), 1);
        assert_eq!(copies(Topping(12)), 12);
        for value in 1..=12u8 {
            assert_eq!(copies(Topping(value)), value as usize, "{}", Topping(value).name());
        }
        for value in [2, 4, 6, 8, 10] {
            assert_eq!(copies(Side(value)), 1, "side {value}");
        }
        assert_eq!(d.iter().filter(|c| matches!(c, Side(_))).count(), 5);
        assert_eq!(copies(FootLong), 1);
        assert_eq!((copies(Fridge), copies(PileOn), copies(Napkin)), (3, 3, 3));
        assert_eq!(d.iter().filter(|c| c.is_action()).count(), 6);
        assert!(!Napkin.is_action() && Fridge.is_action() && PileOn.is_action());
    }

    #[test]
    fn cards_are_named_as_the_table_prints_them() {
        assert_eq!(Topping(12).name(), "Ketchup 12");
        assert_eq!(Topping(0).name(), "Plain Bun 0");
        assert_eq!(Side(2).name(), "Fries +2");
        assert_eq!(Side(10).name(), "Onion Rings +10");
        assert_eq!(FootLong.name(), "Foot-Long ×2");
        assert_eq!(Fridge.name(), "Back in the Fridge");
        assert_eq!(PileOn.name(), "Pile It On");
        assert_eq!(Napkin.name(), "Napkin");
        // A card that cannot be dealt still has a name rather than a panic.
        assert!(!Topping(99).name().is_empty() && !Side(3).name().is_empty());
    }

    #[test]
    fn a_hands_points_double_toppings_with_a_foot_long_then_add_sides_and_the_bonus() {
        assert_eq!(Table::points(&[Topping(12), Topping(5), FootLong, Side(4)], false), 38);
        let seven: Vec<Card> = (0..=6).map(Topping).collect();
        assert_eq!(Table::points(&seven, true), 36);
        assert_eq!(Table::points(&seven, false), 21);
        assert_eq!(Table::points(&[Napkin, Fridge], false), 0);
        assert_eq!(Table::points(&[], false), 0);
    }

    #[test]
    fn the_deal_goes_round_from_the_seat_after_the_dealer_and_the_dealer_moves_each_hand() {
        let mut t = table(5);
        assert_eq!(t.view().hand, 0, "nothing is dealt until the first tick");
        assert!(t.view().events.is_empty());
        deal(&mut t, &DEAL);
        let v = t.view();
        assert_eq!((v.dealer, v.hand, v.turn), (0, 1, Some(1)));
        let dealt: Vec<usize> = events(&t, Kind::Dealt).iter().map(|e| e.seat).collect();
        assert_eq!(dealt, [1, 2, 3, 4, 0]);
        for seat in [1, 2, 3, 4, 0] {
            stop(&mut t, seat);
        }
        assert!(t.view().between);
        assert_eq!(t.view().turn, None);
        t.stack(DEAL.to_vec());
        t.tick(BETWEEN + 0.01);
        settle(&mut t);
        let v = t.view();
        assert_eq!((v.dealer, v.hand, v.turn, v.between), (1, 2, Some(2), false));
        let dealt: Vec<usize> = events(&t, Kind::Dealt).iter().rev().take(5).rev().map(|e| e.seat).collect();
        assert_eq!(dealt, [2, 3, 4, 0, 1]);
        assert!(v.seats.iter().all(|s| s.cards.len() == 1 && s.status == Status::In));
    }

    #[test]
    fn a_duplicate_topping_busts_and_scores_nothing() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Topping(12));
        let v = t.view();
        assert_eq!(v.seats[1].status, Status::Bust);
        assert_eq!(v.seats[1].points, 0);
        assert_eq!(events(&t, Kind::Bust)[0].seat, 1);
        assert_eq!(events(&t, Kind::Bust)[0].card, Some(Topping(12)));
        assert_eq!(v.turn, Some(2), "the turn goes on");
        for seat in [2, 3, 4, 0] {
            stop(&mut t, seat);
        }
        let v = t.view();
        assert!(v.between);
        assert_eq!(v.seats[1].total, 0);
        assert_eq!(v.seats[2].total, 11);
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn a_napkin_wipes_one_duplicate_and_both_go_to_the_discards() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Napkin);
        assert!(t.view().seats[1].napkin);
        for seat in [2, 3, 4, 0] {
            stop(&mut t, seat);
        }
        flip(&mut t, 1, Topping(12));
        let v = t.view();
        assert_eq!(v.seats[1].status, Status::In);
        assert!(!v.seats[1].napkin);
        assert_eq!(v.seats[1].cards, [Topping(12)], "the duplicate is not kept");
        let saved = events(&t, Kind::Saved);
        assert_eq!((saved[0].seat, saved[0].card), (1, Some(Topping(12))));
        assert!(t.discards.contains(&Napkin) && t.discards.contains(&Topping(12)));
        assert_eq!(census(&t), whole_deck());
        assert_eq!(v.turn, Some(1), "it is the only one left in");
    }

    #[test]
    fn a_second_napkin_goes_to_another_hotdog_still_in_without_one_else_it_is_discarded() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Napkin);
        stop(&mut t, 2);
        flip(&mut t, 3, Side(2));
        stop(&mut t, 4);
        stop(&mut t, 0);
        // Seat 1 already holds one; seat 2 has stopped; seat 3 is in and has none.
        flip(&mut t, 1, Napkin);
        let passed = events(&t, Kind::Passed);
        assert_eq!((passed[0].seat, passed[0].target, passed[0].card), (1, Some(3), Some(Napkin)));
        let v = t.view();
        assert!(v.seats[3].napkin && v.seats[1].napkin);
        assert_eq!(v.seats[1].cards.iter().filter(|c| **c == Napkin).count(), 1, "at most one is held");
        // Seat 3 is next, and flips the third: everyone else in already holds one, so it is discarded.
        assert_eq!(v.turn, Some(3));
        let before = t.discards.len();
        flip(&mut t, 3, Napkin);
        let passed = events(&t, Kind::Passed);
        let last = passed.last().unwrap();
        assert_eq!((last.seat, last.target, last.card), (3, None, Some(Napkin)));
        assert_eq!(t.discards.len(), before + 1);
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn seven_different_toppings_are_fully_loaded_and_end_the_hand_for_everyone() {
        let mut t = table(5);
        deal(&mut t, &[Topping(0), Topping(7), Topping(8), Topping(9), Topping(10)]);
        flip(&mut t, 1, Topping(1));
        flip(&mut t, 2, Side(10));
        stop(&mut t, 3);
        stop(&mut t, 4);
        flip(&mut t, 0, Topping(10)); // Relish twice: seat 0 busts
        for (topping, side) in [(2, 8), (3, 6), (4, 4), (5, 2)] {
            flip(&mut t, 1, Topping(topping));
            assert_eq!(t.view().seats[1].status, Status::In);
            flip(&mut t, 2, Side(side));
        }
        assert!(!t.view().between);
        flip(&mut t, 1, Topping(6)); // Plain Bun to Chili: seven different
        let v = t.view();
        assert_eq!(v.seats[1].status, Status::Loaded);
        assert_eq!(v.seats[1].points, 36, "0+1+..+6 and the 15 bonus");
        assert_eq!(v.seats[1].total, 36);
        assert!(v.between, "the hand is over for everyone");
        assert_eq!(v.turn, None);
        assert_eq!(events(&t, Kind::Loaded)[0].seat, 1);
        // Seat 2 was still in: it keeps what it has. The others are as they were.
        assert_eq!((v.seats[2].points, v.seats[2].total), (37, 37));
        assert_eq!((v.seats[3].total, v.seats[4].total), (8, 9));
        assert_eq!((v.seats[0].status, v.seats[0].total), (Status::Bust, 0));
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn back_in_the_fridge_banks_the_target_now() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Fridge);
        assert_eq!(t.view().choosing, Some((1, Fridge)));
        assert_eq!(t.view().turn, Some(1));
        target(&mut t, 1, 2);
        let v = t.view();
        assert_eq!(v.seats[2].status, Status::Fridged);
        assert_eq!((v.seats[2].points, v.seats[2].total), (11, 0), "banked at the hand's end");
        assert_eq!(v.choosing, None);
        let fridged = events(&t, Kind::Fridged);
        assert_eq!((fridged[0].seat, fridged[0].target, fridged[0].card), (1, Some(2), Some(Fridge)));
        assert_eq!(v.turn, Some(3), "the fridged hotdog's turn is skipped");
        for seat in [3, 4, 0, 1] {
            stop(&mut t, seat);
        }
        let v = t.view();
        assert!(v.between);
        let totals: Vec<u32> = v.seats.iter().map(|s| s.total).collect();
        assert_eq!(totals, [8, 12, 11, 10, 9]);
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn pile_it_on_flips_three_then_resolves_actions_drawn_meanwhile() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, PileOn);
        assert_eq!(t.view().choosing, Some((1, PileOn)));
        // Seat 2's three: a Napkin (kept at once), Celery Salt and a Back in the Fridge (dealt with afterwards).
        t.stack(vec![Napkin, Topping(1), Fridge]);
        target(&mut t, 1, 2);
        let v = t.view();
        let flips: Vec<&Event> = v.events.iter().filter(|e| e.kind == Kind::Flipped && e.seat == 2).collect();
        assert_eq!(flips.len(), 3);
        assert_eq!(flips.iter().map(|e| e.card.unwrap()).collect::<Vec<_>>(), [Napkin, Topping(1), Fridge]);
        assert_eq!(v.seats[2].cards, [Topping(11), Napkin, Topping(1)]);
        assert_eq!(v.choosing, Some((2, Fridge)), "the Fridge waits until the three are done");
        let piled = events(&t, Kind::PiledOn);
        assert_eq!((piled[0].seat, piled[0].target), (1, Some(2)));
        assert!(events(&t, Kind::Fridged).is_empty());
        target(&mut t, 2, 3);
        assert_eq!(t.view().seats[3].status, Status::Fridged);
        assert_eq!(t.view().turn, Some(2), "after seat 1's turn comes seat 2's");
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn pile_it_on_stops_at_a_bust_and_discards_actions_drawn_meanwhile() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, PileOn);
        // Seat 2 holds Mustard: a Back in the Fridge, then Mustard again (bust); the third flip is never made.
        t.stack(vec![Fridge, Topping(11), Topping(5)]);
        target(&mut t, 1, 2);
        let v = t.view();
        assert_eq!(v.seats[2].status, Status::Bust);
        assert_eq!(v.events.iter().filter(|e| e.kind == Kind::Flipped && e.seat == 2).count(), 2);
        assert_eq!(v.choosing, None, "the Fridge drawn meanwhile is not played");
        assert!(events(&t, Kind::Fridged).is_empty());
        assert!(t.discards.contains(&Fridge));
        assert_eq!(t.deck.last(), Some(&Topping(5)), "the third flip was not taken");
        assert_eq!(v.turn, Some(3));
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn pile_it_on_stops_when_the_target_becomes_fully_loaded() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, PileOn);
        t.seats[2].cards = (0..6).map(Topping).collect(); // six different toppings already
        t.stack(vec![Fridge, Topping(6), Topping(1)]);
        target(&mut t, 1, 2);
        let v = t.view();
        assert_eq!(v.seats[2].status, Status::Loaded);
        assert_eq!(v.seats[2].points, 36);
        assert_eq!(v.events.iter().filter(|e| e.kind == Kind::Flipped && e.seat == 2).count(), 2);
        assert!(v.between, "fully loaded ends the hand");
        assert_eq!(v.choosing, None);
        assert!(events(&t, Kind::Fridged).is_empty());
        assert!(t.discards.contains(&Fridge));
        assert_eq!(t.deck.last(), Some(&Topping(1)));
        assert_eq!(v.seats[1].total, 12);
    }

    #[test]
    fn a_full_load_ends_the_hand_even_with_another_seats_action_still_queued() {
        for second in [Fridge, PileOn] {
            let mut t = table(5);
            deal(&mut t, &DEAL);
            flip(&mut t, 1, PileOn);
            // Seat 2's three: a Pile It On and a second action card, both put off until the three are flipped, and Fries.
            t.stack(vec![PileOn, second, Side(2)]);
            target(&mut t, 1, 2);
            assert_eq!(t.view().choosing, Some((2, PileOn)));
            // Seat 2 plays its Pile It On on seat 3, which has six different toppings: the first flip fully loads it.
            hold(&mut t, 3, &(0..6).map(Topping).collect::<Vec<_>>());
            t.stack(vec![Topping(6), Side(4), Side(6)]);
            target(&mut t, 2, 3);
            let v = t.view();
            let name = second.name();
            assert_eq!(v.seats[3].status, Status::Loaded, "{name}");
            assert!(v.between, "the hand is over for everyone, whatever is still to be played: {name}");
            assert_eq!(v.choosing, None, "{name}");
            assert_eq!(v.turn, None, "{name}");
            assert_eq!(v.events.iter().filter(|e| e.kind == Kind::Flipped && e.seat == 3).count(), 1, "{name}");
            assert_eq!(events(&t, Kind::PiledOn).len(), 2, "only seat 1's and seat 2's were played: {name}");
            assert!(events(&t, Kind::Fridged).is_empty(), "{name}");
            assert_eq!(t.deck.last(), Some(&Side(4)), "no card was flipped after the full load: {name}");
            assert!(t.jobs.is_empty(), "{name}");
            assert!(t.discards.contains(&second), "the card still waiting was discarded: {name}");
            assert_eq!(census(&t), whole_deck(), "{name}");
            // Seat 2 still banks what it holds: Mustard and Fries.
            assert_eq!(v.seats[2].total, 13, "{name}");
            assert_eq!(v.seats[3].total, 36, "{name}");
        }
    }

    #[test]
    fn turns_skip_hotdogs_that_stopped_busted_or_were_fridged() {
        let mut t = table(5);
        deal(&mut t, &DEAL);
        stop(&mut t, 1);
        flip(&mut t, 2, Topping(11)); // bust
        flip(&mut t, 3, Fridge);
        target(&mut t, 3, 3); // it fridges itself
        let v = t.view();
        assert_eq!(
            [v.seats[1].status, v.seats[2].status, v.seats[3].status],
            [Status::Stopped, Status::Bust, Status::Fridged]
        );
        assert_eq!(v.turn, Some(4));
        flip(&mut t, 4, Side(2));
        assert_eq!(t.view().turn, Some(0));
        flip(&mut t, 0, Side(4));
        assert_eq!(t.view().turn, Some(4), "seats 1, 2 and 3 are skipped");
        flip(&mut t, 4, Side(6));
        assert_eq!(t.view().turn, Some(0));
    }

    #[test]
    fn a_command_out_of_turn_or_at_a_seat_not_in_is_refused() {
        let mut t = table(5);
        assert!(t.command(1, Command::Flip).is_err(), "nothing is dealt yet");
        deal(&mut t, &DEAL);
        let version = t.version();
        for (seat, command) in [
            (2, Command::Flip),
            (2, Command::Stop),
            (0, Command::Flip),
            (1, Command::Target(2)),
            (5, Command::Flip),
            (usize::MAX, Command::Stop),
            (usize::MAX, Command::Target(0)),
        ] {
            assert!(t.command(seat, command).is_err(), "{seat} {command:?}");
        }
        assert_eq!(t.version(), version);
        assert_eq!(t.view().seats[1].cards.len(), 1);
        stop(&mut t, 1);
        flip(&mut t, 2, Fridge);
        let version = t.version();
        assert!(t.command(1, Command::Flip).is_err(), "seat 1 has stopped");
        assert!(t.command(2, Command::Flip).is_err(), "seat 2 has to pick first");
        assert!(t.command(2, Command::Stop).is_err());
        assert!(t.command(3, Command::Target(4)).is_err(), "it is not seat 3's pick");
        assert!(t.command(2, Command::Target(1)).is_err(), "seat 1 is not in the hand");
        assert!(t.command(2, Command::Target(5)).is_err(), "there is no seat 5");
        assert!(t.command(2, Command::Target(usize::MAX)).is_err());
        assert_eq!(t.version(), version);
        assert_eq!(t.view().choosing, Some((2, Fridge)));
        assert!(t.command(2, Command::Target(4)).is_ok());
        assert!(t.command(2, Command::Target(4)).is_err(), "and only once");
    }

    #[test]
    fn at_100_the_highest_total_wins_and_a_tie_plays_one_more_hand_between_the_tied() {
        // Seats 1 and 2 are on 100; the hand adds 5 to each and 1 to the rest.
        let mut t = table(5);
        t.set_total(1, 100);
        t.set_total(2, 100);
        deal(&mut t, &[Topping(5), Topping(5), Topping(1), Topping(1), Topping(1)]);
        for seat in [1, 2, 3, 4, 0] {
            stop(&mut t, seat);
        }
        let v = t.view();
        assert_eq!(v.winner, None, "105 and 105 is a tie");
        assert_eq!(t.winner(), None);
        assert!(v.between);
        assert_eq!(v.seats.iter().map(|s| s.total).collect::<Vec<_>>(), [1, 105, 105, 1, 1]);
        // The tie-break: only the tied are dealt in, from the seat after the new dealer (1).
        t.stack(vec![Topping(12), Topping(3)]);
        t.tick(BETWEEN + 0.01);
        settle(&mut t);
        let v = t.view();
        assert_eq!((v.dealer, v.hand, v.turn), (1, 2, Some(2)));
        assert_eq!(v.seats.iter().map(|s| s.status).collect::<Vec<_>>(), [
            Status::Out,
            Status::In,
            Status::In,
            Status::Out,
            Status::Out
        ]);
        let dealt: Vec<usize> = events(&t, Kind::Dealt).iter().rev().take(2).rev().map(|e| e.seat).collect();
        assert_eq!(dealt, [2, 1]);
        assert!(v.seats[0].cards.is_empty() && v.seats[3].cards.is_empty());
        stop(&mut t, 2);
        stop(&mut t, 1);
        let v = t.view();
        assert_eq!(v.seats.iter().map(|s| s.total).collect::<Vec<_>>(), [1, 108, 117, 1, 1]);
        assert_eq!((v.winner, t.winner()), (Some(2), Some(2)));
        assert_eq!(events(&t, Kind::Won)[0].seat, 2);
        assert!(!v.between);
        assert!(t.command(2, Command::Flip).is_err(), "the game is over");
        t.tick(30.);
        assert_eq!(t.winner(), Some(2));

        // Without a tie the highest total wins, and not before 100: seat 2 leads on 110, seat 1 has 100.
        let mut t = table(5);
        t.set_total(1, 100);
        t.set_total(2, 110);
        deal(&mut t, &[Topping(5), Topping(1), Topping(1), Topping(1), Topping(1)]);
        for seat in [1, 2, 3, 4, 0] {
            stop(&mut t, seat);
        }
        assert_eq!(t.winner(), Some(2));
        // Below 100 the game goes on.
        let mut t = table(5);
        t.set_total(1, 90);
        deal(&mut t, &[Topping(5), Topping(1), Topping(1), Topping(1), Topping(1)]);
        for seat in [1, 2, 3, 4, 0] {
            stop(&mut t, seat);
        }
        assert_eq!(t.winner(), None);
        assert_eq!(t.view().seats[1].total, 95);
    }

    #[test]
    fn a_person_out_of_time_stops_and_a_pick_out_of_time_is_made_for_them() {
        let mut t = table(5);
        t.set_limit(Some(TURN));
        deal(&mut t, &DEAL);
        let clock = t.view().clock.unwrap();
        assert!(clock > 14. && clock <= TURN, "{clock}");
        t.tick(14.);
        assert_eq!(t.view().seats[1].status, Status::In);
        assert!(t.view().clock.unwrap() < 1.1);
        t.tick(1.1);
        let v = t.view();
        assert_eq!(v.seats[1].status, Status::Stopped);
        assert_eq!(v.turn, Some(2));
        assert!(v.clock.unwrap() > 13.5, "seat 2's clock has just started");
        assert_eq!(events(&t, Kind::Stopped)[0].seat, 1);

        // A pick: seat 1 flips a Back in the Fridge and is slow. The bots' choice is made for it: the opponent in
        // with the most at stake, seat 2 (Mustard, 11).
        let mut t = table(5);
        t.set_limit(Some(TURN));
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Fridge);
        assert_eq!(t.view().choosing, Some((1, Fridge)));
        assert!(t.view().clock.unwrap() > 14.);
        t.tick(TURN + 0.1);
        let v = t.view();
        assert_eq!(v.choosing, None);
        assert_eq!(v.seats[2].status, Status::Fridged);
        assert_eq!(v.seats[1].status, Status::In);
    }

    #[test]
    fn a_bot_that_takes_a_persons_seat_mid_decision_acts_within_its_think_time() {
        // Mid-turn.
        let mut t = table(5);
        t.set_limit(Some(TURN));
        deal(&mut t, &DEAL);
        assert_eq!(t.view().turn, Some(1));
        t.stack(vec![Side(2)]);
        t.set_bot(1, true);
        assert!(t.view().seats[1].bot);
        assert_eq!(t.view().clock, None, "a bot's decision has no clock");
        let mut seconds = 0.;
        while t.view().turn == Some(1) {
            t.tick(0.05);
            seconds += 0.05;
            assert!(seconds <= THINK[1] + SHOW + 0.06, "still seat 1's turn after {seconds} s");
        }
        assert!(seconds >= THINK[0], "a bot takes a moment to think: {seconds}");

        // Mid-pick.
        let mut t = table(5);
        t.set_limit(Some(TURN));
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Fridge);
        assert_eq!(t.view().choosing, Some((1, Fridge)));
        t.set_bot(1, true);
        let mut seconds = 0.;
        while t.view().choosing.is_some() {
            t.tick(0.05);
            seconds += 0.05;
            assert!(seconds <= THINK[1] + 0.06, "still picking after {seconds} s");
        }
        assert_eq!(t.view().seats[2].status, Status::Fridged);

        // And a person who takes the seat back has the full clock.
        let mut t = table(5);
        t.set_limit(Some(TURN));
        t.set_bot(1, true);
        deal(&mut t, &DEAL);
        assert_eq!(t.view().turn, Some(1));
        t.set_bot(1, false);
        assert!(t.view().clock.unwrap() > 14.);
    }

    #[test]
    fn when_the_deck_runs_out_the_discards_are_shuffled_in_and_the_table_keeps_its_cards() {
        let mut t = table(2);
        deal(&mut t, &[Topping(3), Topping(2)]); // seat 1: Jalapeños; seat 0: Sport Peppers
        t.stack(vec![Side(2), Side(4), Side(6)]);
        // Everything but those three is in the discards.
        let rest = t.deck.len() - 3;
        let discarded: Vec<Card> = t.deck.drain(..rest).collect();
        t.discards.extend(discarded);
        assert_eq!(t.deck.len(), 3);
        for seat in [1, 0, 1] {
            t.command(seat, Command::Flip).unwrap();
            settle(&mut t);
        }
        assert!(t.deck.is_empty());
        let in_front = t.view().seats[1].cards.clone();
        assert_eq!(in_front, [Topping(3), Side(2), Side(6)]);
        assert_eq!(census(&t), whole_deck());

        t.command(0, Command::Flip).unwrap();
        settle(&mut t);
        let v = t.view();
        assert!(v.deck > 80, "the discards were shuffled into a new deck: {}", v.deck);
        assert_eq!(v.seats[1].cards, in_front, "what lies in front of a seat stays there");
        assert!(v.seats[0].cards.starts_with(&[Topping(2), Side(4)]));
        assert_eq!(events(&t, Kind::Flipped).last().map(|e| e.seat), Some(0));
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn a_flip_with_no_cards_left_anywhere_stops_instead_of_hanging() {
        let mut t = table(2);
        deal(&mut t, &[Topping(3), Topping(2)]);
        t.deck.clear();
        t.discards.clear();
        t.command(1, Command::Flip).unwrap();
        assert_eq!(t.view().seats[1].status, Status::Stopped);
        assert_eq!(t.view().turn, Some(0));
    }

    #[test]
    fn a_bot_stops_when_busting_is_likely_and_flips_when_it_is_not() {
        // Ketchup, Mustard, Relish and Onions in front: the 38 more of them in the 90 left are 42 %.
        let held = [Topping(12), Topping(11), Topping(10), Topping(9)];
        let mut pool = deck();
        for card in held {
            let at = pool.iter().position(|c| *c == card).unwrap();
            pool.remove(at);
        }
        assert!(risk(&held, &pool) > 0.4);
        assert!(!wants_flip(&held, 0, &pool), "no Napkin, a hand worth 42: it stops");
        // The same hand with a Napkin may take the chance (limit 0.30 + 0.25 - 0.10).
        let mut with_napkin = held.to_vec();
        with_napkin.push(Napkin);
        assert!(wants_flip(&with_napkin, 0, &pool));
        // No toppings: nothing to bust on.
        assert_eq!(risk(&[Side(2)], &deck()), 0.);
        assert!(wants_flip(&[Side(2)], 0, &deck()));
        assert!(wants_flip(&[], 0, &deck()));
        // A hand that would win the game by itself is banked at once.
        assert!(!wants_flip(&[Topping(1), Side(10)], 90, &deck()));
        assert!(wants_flip(&[Topping(1), Side(10)], 80, &deck()));
        // An empty hand never "wins", whatever the total.
        assert!(wants_flip(&[Topping(0)], 200, &deck()));
        // An empty deck is not a reason to stop.
        assert_eq!(risk(&held, &[]), 0.);
    }

    #[test]
    fn a_bot_aims_its_actions_at_the_leader() {
        let mut t = table(5);
        for seat in 0..5 {
            t.set_bot(seat, true);
        }
        t.set_total(3, 60);
        t.set_total(4, 40);
        deal(&mut t, &DEAL);
        // Seat 1 is the one picking, with Ketchup 12 (11 in the 88 left: not likely to bust). Seat 3 leads.
        t.seats[1].status = Status::In;
        assert_eq!(t.choose(1, Fridge), 3);
        // Pile It On goes to the one with the most toppings; ties go to the higher total.
        t.seats[2].cards = vec![Topping(1), Topping(2), Topping(3)];
        t.seats[4].cards = vec![Topping(5), Topping(6), Topping(7)];
        assert_eq!(t.choose(1, PileOn), 4);
        // At risk of busting with a good hand it fridges itself.
        t.seats[1].cards = vec![Topping(12), Topping(11), Topping(10), Topping(9)];
        assert_eq!(t.choose(1, Fridge), 1);
        // On its own it is its own target.
        for seat in [0, 2, 3, 4] {
            t.seats[seat].status = Status::Stopped;
        }
        assert_eq!(t.choose(1, PileOn), 1);
        assert_eq!(t.choose(1, Fridge), 1);
    }

    #[test]
    fn bots_play_whole_games_to_one_winner() {
        for seed in 1..=40 {
            let mut t = Table::new(5, seed);
            for seat in 0..5 {
                t.set_bot(seat, true);
            }
            let mut step = 0;
            while t.winner().is_none() {
                t.tick(0.1);
                step += 1;
                assert!(step <= 36000, "seed {seed}: no winner after an hour");
                if step % 50 == 0 {
                    assert_eq!(census(&t), whole_deck(), "seed {seed} at step {step}");
                }
            }
            let v = t.view();
            let winner = v.winner.unwrap();
            assert!(v.seats[winner].total >= TARGET, "seed {seed}");
            for (seat, s) in v.seats.iter().enumerate() {
                if seat != winner {
                    assert!(s.total < v.seats[winner].total, "seed {seed}: seat {seat}");
                }
            }
            assert!(v.hand >= 2, "seed {seed}");
            assert_eq!(census(&t), whole_deck());
        }
    }

    #[test]
    fn the_version_changes_with_the_table_not_with_idle_time() {
        let mut t = table(5);
        assert_eq!(t.version(), 0);
        deal(&mut t, &DEAL);
        let version = t.version();
        assert!(version > 0);
        t.tick(30.);
        assert_eq!(t.version(), version, "waiting on a person changes nothing");
        assert_eq!(t.view().clock, None, "and they have no clock");
        assert_eq!(t.view().version, version);
        t.command(1, Command::Stop).unwrap();
        assert!(t.version() > version);
        // The clock runs without moving the version.
        let mut t = table(5);
        t.set_limit(Some(TURN));
        deal(&mut t, &DEAL);
        let version = t.version();
        t.tick(5.);
        assert_eq!(t.version(), version);
        assert!(t.view().clock.unwrap() < 10.5);
    }

    #[test]
    fn the_version_moves_whenever_what_the_view_shows_moves() {
        for seed in 1..=4 {
            let mut t = Table::new(5, seed);
            for seat in [0, 2, 3, 4] {
                t.set_bot(seat, true);
            }
            t.set_limit(Some(2.));
            t.tick(0.);
            let bare = |v: View| View { clock: None, version: 0, ..v };
            let mut before = (bare(t.view()), t.version());
            let mut moved = 0;
            for _ in 0..30000 {
                t.tick(0.05);
                let now = (bare(t.view()), t.version());
                if now.0 != before.0 {
                    assert_ne!(now.1, before.1, "seed {seed}: the view changed but not the version");
                    moved += 1;
                }
                before = now;
                if t.winner().is_some() {
                    break;
                }
            }
            assert!(moved > 50, "seed {seed}");
            assert!(t.winner().is_some(), "a person on a 2 s clock still finishes the game");
        }
    }

    #[test]
    fn a_view_round_trips_through_json() {
        let mut t = table(5);
        t.set_limit(Some(TURN));
        t.set_bot(3, true);
        deal(&mut t, &DEAL);
        flip(&mut t, 1, Fridge);
        t.tick(0.37);
        let v = t.view();
        assert_eq!(v.choosing, Some((1, Fridge)));
        assert!(v.clock.is_some() && !v.events.is_empty());
        let text = serde_json::to_string(&v).unwrap();
        let back: View = serde_json::from_str(&text).unwrap();
        assert_eq!(back, v);
        // A finished hand, a Foot-Long and all.
        let mut t = table(2);
        deal(&mut t, &[Topping(4), FootLong]);
        stop(&mut t, 1);
        stop(&mut t, 0);
        let v = t.view();
        assert!(v.between);
        let back: View = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn no_command_from_any_seat_can_panic_the_table() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 16
        };
        let seats = [0, 1, 2, 3, 4, 5, 9, usize::MAX];
        let targets = [0, 1, 2, 3, 4, 5, usize::MAX];
        for seed in 1..=6 {
            let mut t = Table::new(5, seed);
            t.set_limit(Some(1.5));
            t.set_bot(3, true);
            for step in 0..6000 {
                let seat = seats[next() as usize % seats.len()];
                let command = match next() % 3 {
                    0 => Command::Flip,
                    1 => Command::Stop,
                    _ => Command::Target(targets[next() as usize % targets.len()]),
                };
                let before = t.version();
                if t.command(seat, command).is_err() {
                    assert_eq!(t.version(), before, "a refused command changes nothing");
                }
                t.set_total(seats[next() as usize % seats.len()], next() as u32 % 90);
                if next() % 8 == 0 {
                    t.set_bot(seats[next() as usize % seats.len()], next() % 2 == 0);
                }
                if next() % 4 == 0 {
                    t.tick((next() % 3000) as f32 / 1000.);
                }
                if step % 500 == 0 {
                    assert_eq!(census(&t), whole_deck(), "seed {seed} step {step}");
                }
            }
            t.tick(f32::NAN);
            t.tick(-5.);
            t.tick(f32::INFINITY);
            assert!(t.view().hand > 0);
        }
    }

    /// A lesson's order for five seats, the person at seat 0: its cards are a topping, two more flips, a side, an action
    /// card and a duplicate; each bot is set a few.
    fn lesson() -> Vec<Vec<Card>> {
        vec![
            vec![Topping(8), Topping(12), Side(2), Fridge, Topping(12)],
            vec![Topping(9), Topping(7)],
            vec![Topping(11), Topping(11)],
            vec![Topping(10)],
            vec![Topping(6), Topping(5)],
        ]
    }

    #[test]
    fn a_lesson_deals_each_seat_its_own_cards_in_order_from_the_dealer_it_names() {
        let mut t = Table::with_order(5, 7, 4, lesson());
        assert_eq!(census(&t), whole_deck(), "the lesson's cards come out of the deck");
        assert_eq!(t.view().deck, 94 - 12);
        settle(&mut t);
        let v = t.view();
        assert_eq!((v.dealer, v.hand, v.turn), (4, 1, Some(0)), "the seat after the dealer is dealt first and goes first");
        let dealt: Vec<(usize, Option<Card>)> = events(&t, Kind::Dealt).iter().map(|e| (e.seat, e.card)).collect();
        assert_eq!(dealt, [(0, Some(Topping(8))), (1, Some(Topping(9))), (2, Some(Topping(11))), (3, Some(Topping(10))), (4, Some(Topping(6)))]);
        // Seat 0's flips are its cards in order, whatever the others do in between.
        for card in [Topping(12), Side(2)] {
            t.command(0, Command::Flip).unwrap();
            settle(&mut t);
            assert_eq!(t.view().seats[0].cards.last(), Some(&card));
            for seat in 1..5 {
                if t.view().turn == Some(seat) {
                    t.command(seat, Command::Flip).unwrap();
                    settle(&mut t);
                }
            }
        }
        // Seat 2 drew its second Mustard: a bust, as the lesson set it. Seat 1 flipped twice: its second card set, then
        // the deck's; seat 3 had only the one card set, so both its flips were the deck's.
        let v = t.view();
        assert_eq!(v.seats[2].status, Status::Bust);
        assert_eq!(v.seats[2].cards, [Topping(11), Topping(11)]);
        assert_eq!(v.seats[1].cards[..2], [Topping(9), Topping(7)]);
        assert_eq!((v.seats[1].cards.len(), v.seats[3].cards.len()), (3, 3));
        assert!(t.order[1].is_empty() && t.order[3].is_empty());
        t.command(0, Command::Flip).unwrap();
        settle(&mut t);
        assert_eq!(t.view().choosing, Some((0, Fridge)));
        target(&mut t, 0, 1);
        assert_eq!(t.view().seats[1].status, Status::Fridged);
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn a_lessons_cards_nobody_drew_go_back_into_the_deck_and_the_next_hand_is_a_games() {
        let mut t = Table::with_order(5, 7, 4, lesson());
        settle(&mut t);
        // Everyone stops at once: seat 0's four flips and seat 1's, 2's and 4's second cards were never drawn.
        for seat in [0, 1, 2, 3, 4] {
            stop(&mut t, seat);
        }
        let v = t.view();
        assert!(v.between);
        assert!(t.order.is_empty(), "the order is over with the hand");
        assert_eq!(v.deck, 94 - 5, "the cards nobody drew are back in the deck");
        assert_eq!(census(&t), whole_deck());
        let ended = events(&t, Kind::Hand)[0].seq;
        t.tick(BETWEEN + 0.01);
        settle(&mut t);
        let v = t.view();
        assert_eq!((v.dealer, v.hand), (0, 2), "the dealer moves on from the lesson's as in a game");
        let dealt: Vec<usize> = events(&t, Kind::Dealt).iter().filter(|e| e.seq > ended).map(|e| e.seat).collect();
        assert_eq!(dealt[0], 1, "{dealt:?}");
    }

    #[test]
    fn clearing_a_lessons_order_puts_its_cards_back_at_once() {
        let mut t = Table::with_order(5, 7, 4, lesson());
        settle(&mut t);
        let version = t.version();
        t.clear_order();
        assert!(t.order.is_empty());
        assert_eq!(t.view().deck, 94 - 5);
        assert_ne!(t.version(), version, "the deck's count moved");
        assert_eq!(census(&t), whole_deck());
        // Nothing to clear: nothing changes.
        let version = t.version();
        t.clear_order();
        assert_eq!(t.version(), version);
        // A card the deck has run out of is left out of an order rather than made twice.
        let t = Table::with_order(2, 7, 0, vec![vec![Topping(0), Topping(0), Napkin], vec![]]);
        assert_eq!(t.order[0], [Topping(0), Napkin]);
        assert_eq!(census(&t), whole_deck());
    }

    #[test]
    fn a_table_with_no_order_plays_as_a_game_does() {
        for seed in [1, 2, 3, 9] {
            let mut game = Table::new(5, seed);
            let mut lesson = Table::with_order(5, seed, 0, vec![]);
            for seat in 0..5 {
                game.set_bot(seat, true);
                lesson.set_bot(seat, true);
            }
            for _ in 0..3000 {
                game.tick(0.1);
                lesson.tick(0.1);
            }
            assert_eq!(game.view(), lesson.view(), "seed {seed}");
            assert!(game.view().hand >= 2, "seed {seed}");
        }
    }

    #[test]
    fn the_pace_slows_the_shows_the_bots_and_the_scores_between_hands() {
        let mut t = Table::with_order(5, 7, 4, lesson());
        t.set_pace(2.);
        t.tick(0.);
        // The first card shows twice as long.
        t.tick(SHOW * 1.5);
        assert_eq!(events(&t, Kind::Dealt).len(), 1);
        t.tick(SHOW * 0.6);
        assert_eq!(events(&t, Kind::Dealt).len(), 2);
        settle(&mut t);
        // A bot thinks twice as long: seat 1 is a bot from its go.
        t.command(0, Command::Stop).unwrap();
        t.set_bot(1, true);
        let Wait::Ask { seat: 1, timer: Some(think), .. } = t.wait else { panic!("{:?}", t.wait) };
        assert!((THINK[0] * 2. ..=THINK[1] * 2.).contains(&think), "{think}");
        for seat in [2, 3, 4] {
            t.set_bot(seat, true);
        }
        for _ in 0..2000 {
            if matches!(t.wait, Wait::Between(_)) {
                break;
            }
            t.tick(0.05);
        }
        let Wait::Between(between) = t.wait else { panic!("{:?}", t.wait) };
        // (Less what was left of the last tick.)
        assert!((BETWEEN * 2. - 0.05..=BETWEEN * 2.).contains(&between), "{between}");
        // Back at a game's pace (and nonsense is a game's pace too).
        t.set_pace(f32::NAN);
        assert_eq!(t.pace, 1.);
        t.set_pace(-1.);
        assert_eq!(t.pace, 1.);
        t.set_pace(1.);
        t.tick(between);
        assert!(matches!(t.wait, Wait::Show(s) if s == SHOW), "{:?}", t.wait);
        // A game's table plays at a game's pace.
        assert_eq!(Table::new(5, 7).pace, 1.);
    }
}

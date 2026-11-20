// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Fully Loaded's tutorial (cards.rs): a guided first hand against the bots, single player only.
//!
//! It is given only when it is asked for: the address's `?tutorial` (natively `--tutorial`, or the word in `--rules`).
//! Players are taken to know the game, so it never starts by itself, not even at a first table (a browser still
//! remembers a lesson seen). Online it is not given: the hand is the server's to deal, and nobody else's game should
//! wait on one player's lesson.
//!
//! **The hand.** The table is laid again from [`order`] (`Table::with_order`): this player is dealt first and has the
//! first go, and its cards come up in the lesson's order (a topping dealt, a second topping, a side, Back in the Fridge,
//! then a topping it already has), while each bot is set cards of its own that keep it clear of action cards and see
//! one bust on its first flip, so the lesson comes out the same whatever the bots or this player decide. The table plays
//! [`PACE`] times as slowly, so that a new player can follow the bots, until the hand ends; then the game goes on as any
//! other.
//!
//! **The steps** ([`Step`]) follow the table's events ([`Lesson::hear`]), never a timer: each waits for this player to
//! act, as single player has no time limit. Off the lesson's path (a stop before the last step, a pick of itself) the
//! steps that can no longer come are passed over, and the last word still comes when the hand ends. What the coach box
//! says at each step is [`coach`]'s; Skip (K, a click on the note, or by touch Stop while the lesson asks for a flip)
//! ends it at once, and the table plays on at a game's pace from a deck no longer set.
use kitchen_sim::cards::{Card, Kind, Status, View};

/// How many times as long the lesson's cards show and its bots think.
pub const PACE: f32 = 1.5;
/// How long (seconds) the last word stays up once the hand is over: through the scores and into the next deal.
pub const WRAP: f32 = 10.;

/// This player's cards in the lesson's hand: dealt Pickles; then Ketchup (a topping and what a bust is), Fries (a side),
/// Back in the Fridge (a pick), and Ketchup again (the duplicate that busts a flip too many).
const MINE: [Card; 5] = [Card::Topping(8), Card::Topping(12), Card::Side(2), Card::Fridge, Card::Topping(12)];
/// The bots' cards, from the seat after this player's round the table. With these the bots flip twice or three times
/// and stop (the second goes bust on its first flip), never past their last card, so that three of them are still in
/// when this player picks who goes back in the fridge.
const BOTS: [&[Card]; 4] = [
    &[Card::Topping(10), Card::Topping(12), Card::Topping(11), Card::Topping(9), Card::Topping(8)],
    &[Card::Topping(11), Card::Topping(11)],
    &[Card::Topping(7), Card::Topping(9), Card::Topping(10), Card::Topping(12), Card::Topping(11)],
    &[Card::Topping(5), Card::Topping(12), Card::Topping(11), Card::Topping(10), Card::Topping(9)],
];

/// The lesson's cards for a table of `seats` with this player at `me`, seat by seat (`Table::with_order`): this
/// player's, and a bot's at each other seat, in turn from the one after it.
pub fn order(seats: usize, me: usize) -> Vec<Vec<Card>> {
    (0..seats)
        .map(|seat| {
            let k = (seat + seats - me % seats.max(1)) % seats.max(1);
            if k == 0 { MINE.to_vec() } else { BOTS[(k - 1) % BOTS.len()].to_vec() }
        })
        .collect()
}
/// The lesson's dealer: the seat before this player's, so that it is dealt first and has the first go.
pub fn dealer(seats: usize, me: usize) -> usize {
    (me + seats.max(1) - 1) % seats.max(1)
}

/// Where the lesson is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// This player's first flip is to come.
    Flip,
    /// Its flip turned up a topping (this one): what a topping is worth, and what a bust is. Flip again.
    Topping(Card),
    /// A side: sides never bust. Flip again.
    Side(Card),
    /// An action card: pick a hotdog to play it on.
    Pick(Card),
    /// Still in after the pick: every flip could match a topping it has. Stop.
    Risk,
    /// Out of the hand (stopped, fridged, bust or fully loaded) while the others finish it.
    Out(Status),
    /// The hand is over (since `at`, real seconds): the last word, for WRAP seconds.
    Wrap { at: f32 },
    /// The lesson is over.
    Done,
}

/// A lesson under way: its step, and the last of the table's events taken in.
#[derive(Clone, Debug, PartialEq)]
pub struct Lesson {
    pub step: Step,
    heard: u64,
}
impl Default for Lesson {
    fn default() -> Self {
        Lesson { step: Step::Flip, heard: 0 }
    }
}
impl Lesson {
    /// Takes in the table's events since the last it heard (`view`, this player at seat `me`, `now` in real seconds),
    /// moving on a step for each that this player's own play brings about, and from the last word to the end once it
    /// has been up for WRAP. A table whose events have started again is not the lesson's: it is over.
    pub fn hear(&mut self, view: &View, me: usize, now: f32) {
        let last = view.events.iter().map(|e| e.seq).max().unwrap_or(0);
        if last < self.heard {
            self.step = Step::Done;
        }
        for event in view.events.iter().filter(|e| e.seq > self.heard) {
            self.step = next(self.step, event, me, now);
        }
        self.heard = self.heard.max(last);
        if matches!(self.step, Step::Wrap { at } if now - at >= WRAP) {
            self.step = Step::Done;
        }
    }
    /// Whether, by touch, Stop skips the lesson rather than stopping: while it asks for a flip (its note says so).
    pub fn stop_skips(&self) -> bool {
        matches!(self.step, Step::Flip | Step::Topping(_) | Step::Side(_))
    }
}
/// The step after `step` once `event` has happened at a table where this player is at `me`.
fn next(step: Step, event: &kitchen_sim::cards::Event, me: usize, now: f32) -> Step {
    let mine = event.seat == me;
    match (event.kind, step) {
        (_, Step::Done | Step::Wrap { .. }) => step,
        (Kind::Hand | Kind::Won, _) => Step::Wrap { at: now },
        (Kind::Flipped, Step::Flip | Step::Topping(_) | Step::Side(_)) if mine => match event.card {
            Some(card @ Card::Topping(_)) if matches!(step, Step::Flip | Step::Topping(_)) => Step::Topping(card),
            Some(card @ (Card::Side(_) | Card::FootLong)) => Step::Side(card),
            Some(card) if card.is_action() => Step::Pick(card),
            _ => step,
        },
        // (Sent back to the fridge, by itself or another: out, its hand banked.)
        (Kind::Fridged, _) if event.target == Some(me) => Step::Out(Status::Fridged),
        (Kind::Fridged | Kind::PiledOn, Step::Pick(_)) if mine => Step::Risk,
        (Kind::Stopped, _) if mine => Step::Out(Status::Stopped),
        (Kind::Bust, _) if mine => Step::Out(Status::Bust),
        (Kind::Loaded, _) if mine => Step::Out(Status::Loaded),
        _ => step,
    }
}

/// What the coach box says: the lesson (`say`), what to do now (`act`, none while there is nothing to do but wait), and
/// how to skip it (`skip`; with the last word up, how to close it; none by touch where Stop is wanted for the game).
#[derive(Clone, Debug, PartialEq)]
pub struct Coach {
    pub say: String,
    pub act: Option<String>,
    pub skip: Option<&'static str>,
}

/// A number from 0 to 12 in words, as a card's points are spoken of.
fn words(n: u32) -> String {
    const WORDS: [&str; 13] = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
    WORDS.get(n as usize).map_or_else(|| n.to_string(), |w| w.to_string())
}
/// "twelve points", "one point", "no points".
fn points(n: u32) -> String {
    format!("{} point{}", words(n), if n == 1 { "" } else { "s" })
}

/// What the coach box says at `step`, at the table `view` with this player at `me`, by mouse or (`touch`) by the touch
/// controls' buttons (Flip, Stop, and Pick and Me while picking). None once the lesson is over.
pub fn coach(step: Step, view: &View, me: usize, touch: bool) -> Option<Coach> {
    let seat = view.seats.get(me)?;
    let (go, picking) = super::asks(view, Some(me));
    let wait = || Some("Wait for your go.".to_string());
    let flip = |again: bool| {
        if !go {
            return wait();
        }
        let card = if again { "again" } else { "a card" };
        Some(if touch { format!("Tap Flip to flip {card}.") } else { format!("Left-click to flip {card}.") })
    };
    let skip = if touch {
        matches!(step, Step::Flip | Step::Topping(_) | Step::Side(_)).then_some("Tap Stop to skip the tutorial")
    } else if matches!(step, Step::Wrap { .. }) {
        Some("Close tutorial (K)")
    } else {
        Some("Skip tutorial (K)")
    };
    let (say, act) = match step {
        Step::Done => return None,
        Step::Flip => {
            let dealt = seat.cards.iter().find_map(|card| match card {
                Card::Topping(value) => Some(format!(": you start with {}, {}", card.name(), points(*value as u32))),
                _ => None,
            });
            (format!("This is Fully Loaded. Each topping is worth its number{}.", dealt.unwrap_or_default()), flip(false))
        }
        Step::Topping(card) => {
            let value = if let Card::Topping(value) = card { value as u32 } else { 0 };
            (format!("{}: {}. The same topping twice ruins your hotdog: a bust.", card.name(), points(value)), flip(true))
        }
        Step::Side(Card::FootLong) => ("Sides never bust you: the Foot-Long doubles your toppings' points.".into(), flip(true)),
        Step::Side(card) => {
            let name = card.name();
            let (side, value) = name.rsplit_once(" +").unwrap_or((name.as_str(), ""));
            let adds = if side.ends_with('s') { "add" } else { "adds" };
            (format!("Sides never bust you: {side} {adds} {value}."), flip(true))
        }
        Step::Pick(card) => {
            let what = match card {
                Card::PileOn => "Pile It On: pick a hotdog. It must flip three cards, one after another.",
                _ => "Back in the Fridge: pick a hotdog. It stops now and banks its points.",
            };
            let how = if touch { "Look at one and tap Pick, or tap Me." } else { "Look at one and click, or press Esc and click a name." };
            (what.to_string(), picking.then(|| how.to_string()))
        }
        Step::Risk => {
            let toppings = seat.cards.iter().filter(|card| matches!(card, Card::Topping(_))).count();
            let have = format!("You have {} topping{}. Every flip could match one.", words(toppings as u32), if toppings == 1 { "" } else { "s" });
            let stop = if touch { "Tap Stop to bank your points." } else { "Right-click to stop and bank your points." };
            (have, if go { Some(stop.to_string()) } else { wait() })
        }
        Step::Out(status) => {
            let say = match status {
                Status::Bust => {
                    let card = seat.cards.last().map_or_else(|| "That topping".to_string(), |card| card.name());
                    format!("{card} again: a bust. Your hotdog scores nothing this hand.")
                }
                Status::Fridged => format!("Back in the fridge: your {} are banked.", points(seat.points)),
                Status::Loaded => format!("FULLY LOADED: seven different toppings, +{}!", kitchen_sim::cards::BONUS),
                _ => format!("You stopped: your {} are banked.", points(seat.points)),
            };
            (say, Some("The others finish the hand.".into()))
        }
        Step::Wrap { .. } => (
            format!(
                "First to {} wins. Seven different toppings is FULLY LOADED: +{} and the hand ends.",
                kitchen_sim::cards::TARGET,
                kitchen_sim::cards::BONUS
            ),
            Some("That's the tutorial. Play on!".into()),
        ),
    };
    Some(Coach { say, act, skip })
}

/// Whether the lesson is to be given at this player's first table offline: only when it is asked for (`asked`), whether
/// or not this browser has seen it (`seen`, None natively, where nothing remembers it).
pub fn due(asked: bool, seen: Option<bool>) -> bool {
    let _ = seen;
    asked
}

/// Whether the lesson has been seen here: in a browser, as its storage remembers; natively nothing is remembered (None).
pub fn seen() -> Option<bool> {
    #[cfg(target_arch = "wasm32")]
    return Some(tutorial_seen());
    #[cfg(not(target_arch = "wasm32"))]
    None
}
/// Remembers that the lesson has been seen (a browser's storage; natively nothing).
pub fn remember() {
    #[cfg(target_arch = "wasm32")]
    tutorial_remember();
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "export function tutorial_seen(){try{return localStorage.getItem('hotdog.cards.tutorial')==='seen'}catch{return false}}
export function tutorial_remember(){try{localStorage.setItem('hotdog.cards.tutorial','seen')}catch{}}")]
extern "C" {
    fn tutorial_seen() -> bool;
    fn tutorial_remember();
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::cards::{Command, Event, Table};

    /// A step without its time, to compare.
    fn label(step: Step) -> String {
        match step {
            Step::Wrap { .. } => "Wrap".into(),
            step => format!("{step:?}"),
        }
    }
    /// The lesson's table for five with this player at seat 0 and bots at the rest.
    fn lesson_table() -> Table {
        let mut t = Table::with_order(5, 3, dealer(5, 0), order(5, 0));
        for seat in 1..5 {
            t.set_bot(seat, true);
        }
        t
    }
    /// Plays the lesson's hand, ten frames a second, with this player at seat 0 doing what `act` says when the table
    /// asks it (given the step and the view), and returns every step the lesson took, what the coach said at each, by
    /// mouse and by touch, at the moment the table asked (or, for a step that asks nothing, when it began), and the
    /// table as the hand ended.
    fn play(act: impl Fn(Step, &View) -> Command) -> (Vec<Step>, Vec<(String, Option<Coach>, Option<Coach>)>, View) {
        let mut t = lesson_table();
        let mut lesson = Lesson::default();
        let (mut steps, mut said) = (vec![lesson.step], vec![]);
        let (mut asked, mut ended) = (None, None);
        for frame in 0..4000 {
            let now = frame as f32 * 0.1;
            t.tick(0.1);
            let v = t.view();
            lesson.hear(&v, 0, now);
            if label(lesson.step) != label(*steps.last().unwrap()) {
                steps.push(lesson.step);
                if label(lesson.step) == "Wrap" {
                    ended = Some(v.clone());
                }
                if !matches!(lesson.step, Step::Topping(_) | Step::Side(_) | Step::Pick(_) | Step::Risk) {
                    said.push((label(lesson.step), coach(lesson.step, &v, 0, false), coach(lesson.step, &v, 0, true)));
                }
            }
            if lesson.step == Step::Done {
                assert_eq!(coach(Step::Done, &v, 0, false), None, "once over, it says nothing");
                // (The last word lasts WRAP seconds from the hand's end: into the next hand's deal.)
                assert_eq!(v.hand, 2);
                return (steps, said, ended.expect("the hand ended"));
            }
            // (Its go is shown while its last card still shows, before the table takes a command: `asks`.)
            let (go, picking) = crate::cards::asks(&v, Some(0));
            if (go || picking) && asked != Some(v.version) && t.command(0, act(lesson.step, &v)).is_ok() {
                asked = Some(v.version);
                said.push((label(lesson.step), coach(lesson.step, &v, 0, false), coach(lesson.step, &v, 0, true)));
            }
        }
        panic!("the lesson never ended: {steps:?}");
    }
    /// What the lesson asks for: flip, pick the first other hotdog still in, stop.
    fn follow(step: Step, v: &View) -> Command {
        match step {
            Step::Pick(_) => Command::Target((1..5).find(|s| v.seats[*s].status == Status::In).unwrap()),
            Step::Risk => Command::Stop,
            _ => Command::Flip,
        }
    }
    fn text(c: &Option<Coach>) -> (String, String, String) {
        let c = c.clone().expect("the coach says something");
        (c.say, c.act.unwrap_or_default(), c.skip.unwrap_or_default().into())
    }

    #[test]
    fn the_lesson_steps_through_its_hand_on_this_players_own_play() {
        let (steps, said, ended) = play(follow);
        let labels: Vec<String> = steps.iter().map(|s| label(*s)).collect();
        assert_eq!(labels, ["Flip", "Topping(Topping(12))", "Side(Side(2))", "Pick(Fridge)", "Risk", "Out(Stopped)", "Wrap", "Done"]);
        // What the coach says when the table asks this player, step by step, by mouse and by touch.
        let at = |step: &str| said.iter().find(|(s, _, _)| s == step).unwrap_or_else(|| panic!("{step}: {said:?}"));
        let (_, mouse, touch) = at("Flip");
        assert_eq!(
            text(mouse),
            (
                "This is Fully Loaded. Each topping is worth its number: you start with Pickles 8, eight points.".into(),
                "Left-click to flip a card.".into(),
                "Skip tutorial (K)".into()
            )
        );
        assert_eq!(text(touch).1, "Tap Flip to flip a card.");
        assert_eq!(text(touch).2, "Tap Stop to skip the tutorial");
        let (_, mouse, touch) = at("Topping(Topping(12))");
        assert_eq!(text(mouse).0, "Ketchup 12: twelve points. The same topping twice ruins your hotdog: a bust.");
        assert_eq!((text(mouse).1.as_str(), text(touch).1.as_str()), ("Left-click to flip again.", "Tap Flip to flip again."));
        let (_, mouse, _) = at("Side(Side(2))");
        assert_eq!(text(mouse).0, "Sides never bust you: Fries add 2.");
        let (_, mouse, touch) = at("Pick(Fridge)");
        assert_eq!(text(mouse).0, "Back in the Fridge: pick a hotdog. It stops now and banks its points.");
        assert_eq!(text(mouse).1, "Look at one and click, or press Esc and click a name.");
        assert_eq!(text(touch).1, "Look at one and tap Pick, or tap Me.");
        assert_eq!(text(touch).2, "", "by touch Stop is Me while picking, not a skip");
        let (_, mouse, touch) = at("Risk");
        assert_eq!(text(mouse).0, "You have two toppings. Every flip could match one.");
        assert_eq!(text(mouse).1, "Right-click to stop and bank your points.");
        assert_eq!(text(touch).1, "Tap Stop to bank your points.");
        let (_, mouse, _) = at("Out(Stopped)");
        assert_eq!(text(mouse).0, "You stopped: your 22 points are banked.");
        assert_eq!((ended.seats[0].status, ended.seats[0].total), (Status::Stopped, 22));
        let (_, mouse, touch) = at("Wrap");
        assert_eq!(text(mouse).0, "First to 100 wins. Seven different toppings is FULLY LOADED: +15 and the hand ends.");
        assert_eq!(text(mouse).1, "That's the tutorial. Play on!");
        assert_eq!(text(mouse).2, "Close tutorial (K)", "nothing left to skip: K closes it");
        assert_eq!(text(touch).2, "", "no skip note by touch once Stop is not a skip");
    }

    #[test]
    fn while_it_is_not_this_players_go_the_coach_says_to_wait() {
        let mut t = lesson_table();
        let mut lesson = Lesson::default();
        // This player's first go comes once the deal is over: nothing else waits.
        for frame in 0..200 {
            t.tick(0.1);
            lesson.hear(&t.view(), 0, frame as f32 * 0.1);
            if t.view().turn == Some(0) {
                break;
            }
        }
        t.command(0, Command::Flip).unwrap();
        // (Once the card has shown, the next seat's go.)
        t.tick(1.);
        lesson.hear(&t.view(), 0, 30.);
        assert_eq!(lesson.step, Step::Topping(Card::Topping(12)));
        assert_ne!(t.view().turn, Some(0));
        assert_eq!(coach(lesson.step, &t.view(), 0, false).unwrap().act.as_deref(), Some("Wait for your go."));
        // The skip note is there all the same.
        assert_eq!(coach(lesson.step, &t.view(), 0, true).unwrap().skip, Some("Tap Stop to skip the tutorial"));
    }

    #[test]
    fn off_the_lessons_path_the_steps_that_cannot_come_are_passed_over() {
        // A flip too many at the risk: the duplicate Ketchup busts this player.
        let (steps, said, _) = play(|step, v| if step == Step::Risk { Command::Flip } else { follow(step, v) });
        assert_eq!(steps.iter().map(|s| label(*s)).collect::<Vec<_>>()[4..], ["Risk", "Out(Bust)", "Wrap", "Done"]);
        let (_, mouse, _) = said.iter().find(|(s, _, _)| s == "Out(Bust)").unwrap();
        assert_eq!(text(mouse).0, "Ketchup 12 again: a bust. Your hotdog scores nothing this hand.");
        assert_eq!(text(mouse).1, "The others finish the hand.");
        // Back in the Fridge played on itself: out, its hand banked; no risk to take.
        let (steps, said, _) = play(|step, v| if let Step::Pick(_) = step { Command::Target(0) } else { follow(step, v) });
        assert_eq!(steps.iter().map(|s| label(*s)).collect::<Vec<_>>()[3..], ["Pick(Fridge)", "Out(Fridged)", "Wrap", "Done"]);
        let (_, mouse, _) = said.iter().find(|(s, _, _)| s == "Out(Fridged)").unwrap();
        assert_eq!(text(mouse).0, "Back in the fridge: your 22 points are banked.");
        // A stop at the first go: no flips to teach, and the last word still comes.
        let (steps, _, _) = play(|_, _| Command::Stop);
        assert_eq!(steps.iter().map(|s| label(*s)).collect::<Vec<_>>(), ["Flip", "Out(Stopped)", "Wrap", "Done"]);
        // A table whose events start again is not the lesson's: it is over.
        let mut lesson = Lesson::default();
        let mut t = lesson_table();
        for _ in 0..40 {
            t.tick(0.1);
        }
        lesson.hear(&t.view(), 0, 4.);
        assert_ne!(lesson.step, Step::Done);
        let mut other = Table::new(5, 1);
        other.tick(0.);
        lesson.hear(&other.view(), 0, 4.1);
        assert_eq!(lesson.step, Step::Done);
    }

    #[test]
    fn an_event_moves_the_lesson_only_when_it_is_this_players() {
        let event = |seat: usize, kind: Kind, card: Option<Card>, target: Option<usize>| Event { seq: 0, seat, kind, card, target };
        // Another hotdog's flip, stop or bust is not this player's lesson.
        for e in [event(2, Kind::Flipped, Some(Card::Topping(3)), None), event(2, Kind::Stopped, None, None), event(2, Kind::Bust, Some(Card::Topping(3)), None)] {
            assert_eq!(next(Step::Flip, &e, 0, 1.), Step::Flip);
        }
        // Another's Back in the Fridge played on this player puts it out; on another, nothing.
        assert_eq!(next(Step::Risk, &event(2, Kind::Fridged, Some(Card::Fridge), Some(0)), 0, 1.), Step::Out(Status::Fridged));
        assert_eq!(next(Step::Risk, &event(2, Kind::Fridged, Some(Card::Fridge), Some(3)), 0, 1.), Step::Risk);
        // A Foot-Long is a side to the lesson; a Napkin moves nothing; Pile It On is a pick too.
        assert_eq!(next(Step::Topping(Card::Topping(12)), &event(0, Kind::Flipped, Some(Card::FootLong), None), 0, 1.), Step::Side(Card::FootLong));
        assert_eq!(next(Step::Flip, &event(0, Kind::Flipped, Some(Card::Napkin), None), 0, 1.), Step::Flip);
        assert_eq!(next(Step::Side(Card::Side(2)), &event(0, Kind::Flipped, Some(Card::PileOn), None), 0, 1.), Step::Pick(Card::PileOn));
        // Fully loaded; and the hand's end from any step, once.
        assert_eq!(next(Step::Risk, &event(0, Kind::Loaded, Some(Card::Topping(1)), None), 0, 1.), Step::Out(Status::Loaded));
        assert_eq!(next(Step::Flip, &event(4, Kind::Hand, None, None), 0, 7.), Step::Wrap { at: 7. });
        assert_eq!(next(Step::Wrap { at: 7. }, &event(4, Kind::Hand, None, None), 0, 9.), Step::Wrap { at: 7. });
        assert_eq!(next(Step::Done, &event(0, Kind::Flipped, Some(Card::Topping(2)), None), 0, 9.), Step::Done);
        // By touch Stop skips only while the lesson asks for a flip.
        let skips = |step: Step| Lesson { step, heard: 0 }.stop_skips();
        assert!(skips(Step::Flip) && skips(Step::Topping(Card::Topping(12))) && skips(Step::Side(Card::Side(2))));
        assert!(!skips(Step::Pick(Card::Fridge)) && !skips(Step::Risk) && !skips(Step::Out(Status::Stopped)) && !skips(Step::Wrap { at: 0. }));
    }

    #[test]
    fn the_lessons_cards_come_up_whatever_this_player_picks_and_the_bots_stay_within_theirs() {
        let mine = order(5, 0)[0].clone();
        for pick in 0..5 {
            for flip_at_risk in [false, true] {
                let (steps, _, v) = play(|step, v| match step {
                    Step::Pick(_) => {
                        // (In the hand at the pick: this player and at least two others to choose from.)
                        let in_hand: Vec<usize> = (0..5).filter(|s| v.seats[*s].status == Status::In).collect();
                        assert!(in_hand.len() >= 3, "{in_hand:?}");
                        Command::Target(if in_hand.contains(&pick) { pick } else { in_hand[1] })
                    }
                    Step::Risk if flip_at_risk => Command::Flip,
                    step => follow(step, v),
                });
                assert!(steps.contains(&Step::Pick(Card::Fridge)), "pick {pick}: {steps:?}");
                // Each bot drew only cards it was set, and none was fully loaded; this player's were its own, in order.
                assert_eq!(v.hand, 1);
                for seat in 1..5 {
                    let set = &order(5, 0)[seat];
                    assert!(set.starts_with(&v.seats[seat].cards), "pick {pick} seat {seat}: {:?}", v.seats[seat].cards);
                    assert_ne!(v.seats[seat].status, Status::Loaded);
                }
                // (Back in the Fridge, played, is in the discards.)
                let kept: Vec<Card> = mine.iter().copied().filter(|card| !card.is_action()).collect();
                assert!(kept.starts_with(&v.seats[0].cards), "{:?}", v.seats[0].cards);
                assert_eq!(v.seats[0].status, if pick == 0 { Status::Fridged } else if flip_at_risk { Status::Bust } else { Status::Stopped });
            }
        }
        // Seated anywhere, this player has its own cards and is dealt first; each bot has a set of its own.
        for me in 0..5 {
            let o = order(5, me);
            assert_eq!(o[me], mine);
            assert_eq!(dealer(5, me), (me + 4) % 5);
            assert_eq!(o[(me + 2) % 5], [Card::Topping(11), Card::Topping(11)], "the bot that busts is two seats on");
        }
    }

    #[test]
    fn the_lesson_is_due_only_when_asked() {
        // Any run that asks for it: given, seen or not.
        assert!(due(true, Some(false)));
        assert!(due(true, Some(true)));
        assert!(due(true, None));
        // Not asked: never, not even in a browser that has not seen it (it does not start by itself).
        assert!(!due(false, Some(false)));
        assert!(!due(false, Some(true)));
        assert!(!due(false, None));
        assert_eq!(seen(), None);
        // `?tutorial` (a word of the address), natively `--tutorial` or the word in `--rules`.
        let asked = |params: &str, tutorial: bool| crate::runtime::Options { params: params.into(), tutorial, ..Default::default() }.tutorial();
        assert!(asked("play mode=cards tutorial", false));
        assert!(asked("tutorial", false));
        assert!(asked("play&mode=cards&tutorial=1", false));
        assert!(asked("mode=cards", true));
        assert!(!asked("play mode=cards", false));
        assert!(!asked("tutorials mode=cards", false));
        assert!(!asked("", false));
    }
}

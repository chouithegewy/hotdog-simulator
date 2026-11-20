// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! One map's game, as the server plays it: kitchen-sim's `Feast` (every rule of a round, the same simulation single
//! player and the client's prediction run) with what only a server needs round it.
//!
//! - **The lobby's stages.** Between rounds the stage is `Lobby`: everyone waits in the closed fridge and nothing on
//!   the counter moves. Once every seated player is ready a countdown runs (it stops if one of them changes their mind
//!   or somebody new sits down); then the stage is `Opening`, the start sequence every client plays; then a round
//!   begins with its warm-up (Forts: its build phase), and a finished round (Forts: match) returns to the lobby.
//! - **Seats.** The hotdogs are the map's five (six in Forts), always: a seat is `connected` while somebody plays it
//!   and a bot plays it otherwise, `waiting` while that somebody arrived during a round and sits it out in the fridge
//!   (a bot goes on playing the slot). Joining takes over a bot's seat; a bot takes over one its player has left.
//! - **Commands.** A player's client sends what its player does, a numbered command a step (kitchen_sim::net), not
//!   where its hotdog stands. Each seat queues them (`commands`); every step applies the next (two while a backlog is
//!   being caught up, never more than one a step over time) with the simulation's own movement and actions, as single
//!   player steers its hotdog (`command`), and remembers the last applied, which the frames acknowledge.
//! - **Dress.** Each hotdog's name and colour are its player's while its player plays it, and the bots' own otherwise.
//! - **The frame**, what every client is told of the game (net.rs `Frame`).
//! - **Rules** (kitchen_sim::rules). Free For All is played through the lobby as a siege is, except that somebody
//!   arriving during a round sits down at once (the dead come back anyway). The dev sandbox has no lobby at all: the
//!   round starts as soon as somebody sits down, never ends, and everyone who arrives plays at once.
//! - **Fully Loaded** (kitchen_sim::cards) is played through the lobby too: the hotdogs stand at the card table and a
//!   person's walking, stances and guns do nothing there (`drive`, `command`); what they do is flip, stop or choose
//!   a hotdog (`cards`), and only for their own seat. Each game's table is shuffled differently (`with_rules`).
//! - **Kung Fu** (kitchen_sim::kungfu) is played through the lobby as Free For All is; a person's command goes whole to
//!   `Feast::kungfu_command`, the same function single player calls (`command`).
use kitchen_sim::cards;
use kitchen_sim::feast::{Feast, NAMES, Player, Stage};
use kitchen_sim::forts;
use kitchen_sim::map::Map;
use kitchen_sim::net::{self, Command, Frame, Loose, Seat as Shown, button};
use kitchen_sim::rules::Rules;
use std::collections::VecDeque;

pub use kitchen_sim::net::Profile;

/// Seconds the countdown and the start sequence last unless the server says otherwise (--countdown, --opening).
pub const COUNTDOWN: f32 = 3.;
pub const OPENING: f32 = 9.;
/// A step of the game at the server's usual rate (--tick says otherwise).
pub const STEP: f32 = 1. / net::TICK_HZ as f32;
/// Commands. A seat keeps at most QUEUE waiting (the oldest goes first, its presses kept for the next) and takes
/// none numbered more than AHEAD past the newest it has had. It applies one a step, two while more than BACKLOG
/// wait, and never more than one a step over time: it earns RATE commands a step (a little over one, for a client
/// whose clock runs a little fast) and saves at most BUDGET.
pub const QUEUE: usize = 32;
pub const AHEAD: u32 = 64;
pub const BACKLOG: usize = 2;
pub const RATE: f32 = 1.02;
pub const BUDGET: f32 = 8.;
/// A person's hotdog takes one step of the simulation for each of its player's commands, as the player's own
/// prediction steps it: in a step that applies none it is held still (it does not fall or fly meanwhile), for HANG
/// seconds at most; a player silent for longer is stepped as the rest of the round is.
pub const HANG: f32 = 0.1;
/// The five colours a player may choose for their hotdog, which wears it on the ring round its waist and on its name
/// tag; a bot, and every hotdog without a lobby, is BARE: the colour of its place.
pub const COLOURS: [&str; 5] = ["red", "blue", "green", "yellow", "black"];
pub const BARE: &str = "";

/// How the server plays every game: the command line's choices.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Bots play round after round for whoever watches; there is no lobby.
    pub demo: bool,
    pub countdown: f32,
    pub opening: f32,
    /// Forts: seconds the build phase lasts instead of the map's, and a fight may stay quiet before the cats come.
    pub build: Option<f32>,
    pub stale: Option<f32>,
    /// A join may ask for the dev sandbox (--dev).
    pub dev: bool,
    /// The tuning every game starts with (--tune, kitchen_sim::tune's text; empty for the constants).
    pub tune: String,
    /// How well the bots aim in every game whose rules do not say (--bots; kitchen_sim::aim): normal unless asked.
    pub skill: kitchen_sim::aim::Skill,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { demo: false, countdown: COUNTDOWN, opening: OPENING, build: None, stale: None, dev: false, tune: String::new(), skill: Default::default() }
    }
}
/// Where the game is: in the fridge between rounds, opening it, or playing a round (whose own stage the Feast keeps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Lobby,
    Opening,
    Round,
}
/// What the server knows of a hotdog's seat beyond the round: who plays it and how they look and sound.
#[derive(Clone, Debug)]
struct Seat {
    connected: bool,
    waiting: bool,
    profile: Option<Profile>,
    talk: f32,
    /// Forts: the team its player asked for (Some(None): left to the server), once the lobby has said.
    wish: Option<Option<u8>>,
    /// The colour it wears now (BARE for a bot).
    color: String,
    /// Its player's commands waiting, oldest first; the number of the newest it has had and of the last applied; and
    /// how many it may still apply (see QUEUE).
    queue: VecDeque<Command>,
    newest: u32,
    applied: u32,
    budget: f32,
    /// Seconds since a command was last applied.
    silent: f32,
}
impl Seat {
    fn empty() -> Seat {
        Seat { connected: false, waiting: false, profile: None, talk: 0., wish: None, color: BARE.into(), queue: VecDeque::new(), newest: 0, applied: 0, budget: 1., silent: 0. }
    }
}

pub struct Game {
    pub feast: Feast,
    /// The map, and its canonical text, which every client is sent.
    pub map: Map,
    pub text: String,
    seats: Vec<Seat>,
    /// Whether anybody has joined since the game was last empty (with nobody, nothing moves and no hotdog is listed).
    occupied: bool,
    pub phase: Phase,
    /// Seconds left of the countdown while it runs, and seconds into the start sequence.
    pub countdown: Option<f32>,
    pub opening: f32,
    pause: [f32; 2],
    /// Whether there is a lobby (there is, unless the server plays the demo).
    lobby: bool,
    demo: bool,
}

impl Game {
    /// A game on `map`, in the lobby (in the demo: playing).
    pub fn new(map: Map, settings: &Settings) -> Game {
        Game::with_rules(map, settings, Rules::default())
    }
    /// A game on `map` played by `rules`: in the lobby, or with the sandbox's (or in the demo) playing.
    pub fn with_rules(map: Map, settings: &Settings, mut rules: Rules) -> Game {
        // (The bots' skill: the rules' own, or else the server's.)
        if rules.skill == kitchen_sim::aim::Skill::Normal {
            rules.skill = settings.skill;
        }
        let text = map.to_text();
        let mut feast = Feast::with_map(map.clone(), settings.demo);
        feast.follow = true;
        if let Some(seconds) = settings.build {
            feast.set_build(seconds);
        }
        if let Some(seconds) = settings.stale {
            feast.set_stale(seconds);
        }
        let lobby = !settings.demo && !rules.sandbox;
        feast.tune = kitchen_sim::tune::Tuning::parse(&settings.tune);
        // (Fully Loaded: the table's shuffle comes from the seed and the round, and the seed outlasts the rounds, so
        // it is a different one for every game. It is read when the rules set the table out.)
        feast.card_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos() as u64);
        feast.set_rules(rules);
        let seats = vec![Seat::empty(); feast.players.len()];
        let mut game = Game {
            feast,
            map,
            text,
            seats,
            occupied: false,
            phase: Phase::Round,
            countdown: None,
            opening: 0.,
            pause: [settings.countdown, settings.opening],
            lobby,
            demo: settings.demo,
        };
        // Nobody has joined: every hotdog is a bot.
        let keep = game.keep();
        game.refit(&keep, None);
        if game.lobby {
            game.park();
        }
        game
    }
    /// How many hotdogs a round has: five, or six in Forts.
    pub fn seats(&self) -> usize {
        self.seats.len()
    }
    /// How many hotdogs are listed: none until somebody joins.
    pub fn players(&self) -> usize {
        if self.occupied { self.seats.len() } else { 0 }
    }
    pub fn player(&self, id: u32) -> Option<&Player> {
        self.feast.player(id)
    }
    /// The seat a hotdog (by its id) sits at: its place among the round's players, which is the seat the card table
    /// knows it by.
    pub fn seat(&self, id: u32) -> Option<usize> {
        self.feast.players.iter().position(|p| p.id == id)
    }
    fn index(&self, id: u32) -> Option<usize> {
        (id as usize).checked_sub(1).filter(|i| *i < self.seats.len())
    }
    pub fn waiting(&self, id: u32) -> bool {
        self.index(id).is_some_and(|i| self.seats[i].waiting)
    }
    pub fn connected(&self, id: u32) -> bool {
        self.index(id).is_some_and(|i| self.seats[i].connected)
    }
    pub fn talking(&self, id: u32) -> f32 {
        self.index(id).map_or(0., |i| self.seats[i].talk)
    }
    /// The stage as clients are told it: Lobby, Opening, or the round's own.
    pub fn stage(&self) -> &'static str {
        match self.phase {
            Phase::Lobby => "Lobby",
            Phase::Opening => "Opening",
            Phase::Round => stage_name(self.feast.stage),
        }
    }
    pub fn forts(&self) -> bool {
        self.feast.fort.is_some()
    }
    /// What the game is played by beyond its map (kitchen_sim::rules).
    pub fn rules(&self) -> &Rules {
        &self.feast.rules
    }

    // ----- Seats.

    /// Somebody sits down: in the lobby (or in the demo, at once) they take over the first seat nobody plays; during
    /// a round they are given a seat for the next one (`queue`). None when every seat is taken.
    pub fn join(&mut self) -> Option<u32> {
        // (Free For All's round takes newcomers at once: they come back when they die anyway.)
        if self.lobby && self.phase != Phase::Lobby && !(self.phase == Phase::Round && self.feast.rules.ffa) {
            return self.queue();
        }
        let i = self.seats.iter().position(|seat| !seat.connected)?;
        self.occupied = true;
        self.seats[i].connected = true;
        let id = i as u32 + 1;
        self.feast.players[i].bot = self.demo;
        self.reset_player(id);
        self.dress();
        Some(id)
    }
    /// Somebody arriving during a round (or its start sequence) is given a seat for the next one: the bot keeps playing
    /// it until then.
    fn queue(&mut self) -> Option<u32> {
        let i = self.seats.iter().position(|seat| !seat.connected)?;
        (self.seats[i].connected, self.seats[i].waiting) = (true, true);
        self.feast.players[i].ready = false;
        self.dress();
        Some(i as u32 + 1)
    }
    /// A seat's player has gone: a bot takes over. An empty game goes back to the start, and with a lobby to it.
    pub fn leave(&mut self, id: u32) {
        if let Some(i) = self.index(id) {
            self.vacate(i);
            self.seats[i].connected = false;
            let p = &mut self.feast.players[i];
            (p.bot, p.prone, p.crouch) = (true, false, false);
        }
        if !self.seats.iter().any(|seat| seat.connected) {
            // Nobody is left: the game starts again from nothing, every hotdog a bot on its own team.
            self.occupied = false;
            self.seats = vec![Seat::empty(); self.seats.len()];
            self.feast.reset();
            let fresh = self.keep();
            self.refit(&fresh, None);
        }
        self.adjourn();
    }
    /// A seat its player has left: what it knew of that player goes with them.
    fn vacate(&mut self, i: usize) {
        let seat = &mut self.seats[i];
        (seat.waiting, seat.talk, seat.profile, seat.wish) = (false, 0., None, None);
        (seat.queue, seat.newest, seat.applied, seat.budget) = (VecDeque::new(), 0, 0, 1.);
        self.feast.players[i].ready = false;
    }
    /// After somebody has left: an empty game goes back to the lobby, and so does a round nobody seated is left in
    /// (those waiting need not sit through the bots' game).
    fn adjourn(&mut self) {
        if !self.lobby {
            return;
        }
        if !self.occupied {
            self.park();
        } else if self.phase != Phase::Lobby && self.seated().is_empty() {
            self.restart(None);
            self.park();
        }
    }
    /// The seats the next round waits for: somebody plays them and is not sitting a round out.
    fn seated(&self) -> Vec<usize> {
        (0..self.seats.len()).filter(|i| self.seats[*i].connected && !self.seats[*i].waiting).collect()
    }
    /// A hotdog as a round begins (taken over by somebody, or handed back): see `Feast::fit`.
    fn reset_player(&mut self, id: u32) {
        self.feast.fit(id);
    }
    /// What the players keep across a new round: their team and their ready mark.
    fn keep(&self) -> Vec<(u8, bool)> {
        self.feast.players.iter().map(|p| (p.team, p.ready)).collect()
    }
    /// A new round has been set out (`Feast::reset`): every hotdog is put back on its seat as it was, a person's or a
    /// bot's, on its team (`teams`, or what it had), and fitted out for the round. In Forts whoever was waiting for a
    /// round takes their seat; in a siege the ready marks stay until the lobby.
    fn refit(&mut self, keep: &[(u8, bool)], teams: Option<&[u8]>) {
        let forts = self.feast.fort.is_some();
        for i in 0..self.seats.len() {
            if forts || self.feast.rules.ffa {
                self.seats[i].waiting = false;
            }
            let seat = &self.seats[i];
            let p = &mut self.feast.players[i];
            p.bot = self.demo || !seat.connected || seat.waiting;
            if let Some(&(team, ready)) = keep.get(i) {
                if forts {
                    p.team = teams.and_then(|teams| teams.get(i).copied()).unwrap_or(team);
                } else {
                    p.ready = ready;
                }
            }
        }
        for id in 1..=self.seats.len() as u32 {
            self.reset_player(id);
        }
        if forts {
            self.feast.herald();
        }
    }
    /// The next round, set out afresh on the same map (`teams` for a Forts round's sides).
    fn restart(&mut self, teams: Option<&[u8]>) {
        let keep = self.keep();
        self.feast.reset();
        self.refit(&keep, teams);
    }

    // ----- The lobby's stages.

    /// Into the lobby (after a new round has been set out): nobody is ready, and those who were waiting take their
    /// seats. A Forts match still under way is over.
    pub fn park(&mut self) {
        if let Some(fort) = &mut self.feast.fort {
            if fort.champion.is_some() || fort.scores != [0, 0] {
                fort.match_no += 1;
            }
            (fort.scores, fort.champion) = ([0, 0], None);
        }
        (self.phase, self.countdown, self.opening) = (Phase::Lobby, None, 0.);
        for i in 0..self.seats.len() {
            self.feast.players[i].ready = false;
            if self.seats[i].waiting {
                self.seats[i].waiting = false;
                self.feast.players[i].bot = self.demo;
            }
        }
        self.dress();
        self.announce();
    }
    /// One step of the lobby. Every seated player ready: the countdown runs, then the fridge opens, then the round
    /// begins.
    fn linger(&mut self, dt: f32) {
        self.feast.elapsed += dt;
        self.dress();
        if self.phase == Phase::Opening {
            self.opening += dt;
            if self.opening >= self.pause[1] {
                self.begin();
            }
            return;
        }
        let seated = self.seated();
        if !seated.is_empty() && seated.iter().all(|i| self.feast.players[*i].ready) {
            let left = self.countdown.unwrap_or(self.pause[0]) - dt;
            self.countdown = Some(left);
            if left <= 0. {
                self.open(0.);
            }
        } else {
            self.countdown = None;
        }
        self.announce();
    }
    /// The start sequence begins, `at` seconds into it: from the top when the countdown runs out, or where it was
    /// when a fresh game takes over a lobby whose leader chose another map.
    pub fn open(&mut self, at: f32) {
        (self.countdown, self.phase, self.opening) = (None, Phase::Opening, at);
        self.dress();
        self.announce();
    }
    fn announce(&mut self) {
        let seated = self.seated();
        let ready = seated.iter().filter(|i| self.feast.players[**i].ready).count();
        self.feast.alert = match (self.phase, self.countdown) {
            (Phase::Opening, _) => "THE FRIDGE OPENS. THE PACKAGE IS OPENED.".into(),
            (_, Some(left)) => format!("THE FRIDGE OPENS IN {}", left.ceil().max(1.)),
            _ => format!("IN THE FRIDGE. {ready} OF {} READY.", seated.len()),
        };
    }
    /// The round begins: Forts sides are made up, everything is set out afresh and the warm-up (build phase) runs.
    fn begin(&mut self) {
        let teams = self.feast.fort.is_some().then(|| self.muster());
        self.restart(teams.as_deref());
        (self.phase, self.countdown, self.opening) = (Phase::Round, None, 0.);
        self.dress();
    }
    /// Forts sides, three each: a player's team is the one they asked for while it has room, the smaller one
    /// otherwise; the bots make up the numbers. By seat.
    fn muster(&self) -> Vec<u8> {
        let half = (self.seats.len() / 2).max(1);
        let (mut count, mut team) = ([0usize; 2], vec![None; self.seats.len()]);
        let seat = |i: usize, mut t: usize, count: &mut [usize; 2], team: &mut Vec<Option<u8>>| {
            if count[t] >= half {
                t = 1 - t;
            }
            team[i] = Some(t as u8);
            count[t] += 1;
        };
        let people: Vec<usize> = (0..self.seats.len()).filter(|i| self.seats[*i].connected && !self.feast.players[*i].bot).collect();
        for &i in &people {
            if let Some(Some(t)) = self.seats[i].wish {
                seat(i, t as usize, &mut count, &mut team);
            }
        }
        for i in people.into_iter().chain(0..self.seats.len()) {
            if team[i].is_none() {
                let t = if count[0] <= count[1] { 0 } else { 1 };
                seat(i, t, &mut count, &mut team);
            }
        }
        team.into_iter().map(|t| t.unwrap_or(0)).collect()
    }
    /// A seated player says whether they are ready (the lobby listens, and a Forts build phase). Returns whether it
    /// was taken.
    pub fn ready(&mut self, id: u32, flag: bool) -> bool {
        if self.phase == Phase::Round && self.feast.stage == Stage::Build {
            return self.feast.ready(id, flag);
        }
        let Some(i) = self.index(id) else {
            return false;
        };
        if self.phase != Phase::Lobby || !self.seats[i].connected || self.seats[i].waiting {
            return false;
        }
        self.feast.players[i].ready = flag;
        true
    }
    /// Forts: the team a player asks to be on, 0 or 1; anything else leaves it to the server. Returns whether there
    /// are teams to choose.
    pub fn team(&mut self, id: u32, wish: Option<u8>) -> bool {
        if self.feast.fort.is_none() {
            return false;
        }
        if let Some(i) = self.index(id) {
            self.seats[i].wish = Some(wish.filter(|t| *t <= 1));
        }
        true
    }
    /// The name and the colour a player chose.
    pub fn profile(&mut self, id: u32, profile: Profile) {
        if let Some(i) = self.index(id) {
            self.seats[i].profile = Some(profile);
        }
        self.dress();
    }
    /// How loudly a player is speaking, 0 to 1: the hotdog's mouth moves with it.
    pub fn talk(&mut self, id: u32, level: f32) {
        if let Some(i) = self.index(id) {
            let seat = &mut self.seats[i];
            seat.talk = if seat.connected { level.clamp(0., 1.) } else { 0. };
        }
    }
    /// What each hotdog is called and the colour it wears: its player's profile while that player plays it, and
    /// otherwise (a bot, a seat whose player waits for the next round, a demo's spectator) the built-in name and BARE.
    fn dress(&mut self) {
        let forts = self.feast.fort.is_some();
        for i in 0..self.seats.len() {
            let (team, id) = (self.feast.players[i].team, self.feast.players[i].id);
            let seat = &mut self.seats[i];
            let mine = if seat.connected && !self.feast.players[i].bot { seat.profile.as_ref() } else { None };
            let name = match mine {
                Some(profile) => profile.name.clone(),
                None if forts => {
                    let rank = self.feast.players.iter().filter(|q| q.team == team && q.id < id).count();
                    forts::NAMES[team.min(1) as usize][rank % 3].to_string()
                }
                None => NAMES[i % NAMES.len()].to_string(),
            };
            seat.color = mine.map_or(BARE.into(), |profile| profile.color.clone());
            if !seat.connected {
                seat.talk = 0.;
            }
            self.feast.players[i].name = name;
        }
    }

    // ----- Steps and commands.

    /// One step of the game, `dt` seconds: of the lobby between rounds, of the round during one, each player's next
    /// command applied first. Nothing moves while nobody plays.
    pub fn tick(&mut self, dt: f32) {
        self.drive(dt);
        if !self.occupied {
            return;
        }
        if self.phase != Phase::Round {
            return self.linger(dt);
        }
        self.dress();
        let (keep, round) = (self.keep(), self.feast.round);
        let champion = self.feast.fort.as_ref().is_some_and(|fort| fort.champion.is_some());
        self.feast.tick(dt);
        if self.feast.round != round {
            // The round set out the next by itself: after a siege round, or between the rounds of a Forts match. After
            // the round, or the match, everyone goes back to the fridge.
            self.refit(&keep, None);
            if self.lobby && (self.feast.fort.is_none() || champion) {
                self.park();
            }
        }
    }
    /// A player presses use at the griddle in reach of their hotdog (kitchen_sim::griddle): it flips, on a round that is on (not in
    /// the fridge). Returns whether one did.
    pub fn use_griddle(&mut self, id: u32) -> bool {
        self.phase == Phase::Round && self.feast.use_griddle(id)
    }
    /// A hotdog's player's commands, as they came (net.rs `Batch`): queued in order. One the seat has had already (a
    /// batch repeats the newest few) is passed over, and so is one numbered too far ahead (AHEAD) of the newest it
    /// has had. Too many waiting (QUEUE): the oldest goes, and what was pressed in it is kept for the next.
    pub fn commands(&mut self, id: u32, commands: &[Command]) {
        let Some(i) = self.index(id) else {
            return;
        };
        let seat = &mut self.seats[i];
        for c in commands {
            if c.seq <= seat.newest || c.seq > seat.newest.saturating_add(AHEAD) && seat.newest > 0 {
                continue;
            }
            seat.newest = c.seq;
            seat.queue.push_back(*c);
            if seat.queue.len() > QUEUE {
                let dropped = seat.queue.pop_front().unwrap();
                seat.applied = dropped.seq;
                if let Some(next) = seat.queue.front_mut() {
                    next.buttons |= dropped.buttons & button::EDGES;
                    // (A reload is an edge too, though not a bit of its own: `button::RELOAD`.)
                    if dropped.reloads() {
                        next.buttons = next.buttons & !(button::TURN | button::TIP) | button::RELOAD;
                    }
                    next.pickup = next.pickup.or(dropped.pickup);
                }
            }
        }
    }
    /// The number of the last of a hotdog's commands applied (0 for none): what its client's prediction starts again
    /// from.
    pub fn applied(&self, id: u32) -> u32 {
        self.index(id).map_or(0, |i| self.seats[i].applied)
    }
    /// How many of a hotdog's commands are waiting.
    pub fn waiting_commands(&self, id: u32) -> usize {
        self.index(id).map_or(0, |i| self.seats[i].queue.len())
    }
    /// Applies the commands due this step (see QUEUE): during a round, to each living hotdog a person plays; otherwise
    /// (the lobby, the start, a seat waiting for the next round, a bot's, the dead, after the round) they are taken as
    /// read and do nothing. A person's hotdog takes a step for each command (HANG): the step of the round for the last
    /// applied, a step of its own for one before it, none at all (held still) when none was.
    fn drive(&mut self, dt: f32) {
        // (At the card table nobody walks, crouches or fires: what a person does there is `cards`.)
        let playing = self.phase == Phase::Round && self.feast.stage != Stage::RoundOver && self.feast.cards.is_none();
        for i in 0..self.seats.len() {
            let id = i as u32 + 1;
            let steering = playing && self.seats[i].connected && !self.seats[i].waiting && !self.feast.players[i].bot && self.feast.players[i].alive;
            let seat = &mut self.seats[i];
            seat.budget = (seat.budget + RATE).min(BUDGET);
            if !steering {
                if let Some(last) = seat.queue.back() {
                    seat.applied = last.seq;
                }
                (seat.queue, seat.silent) = (VecDeque::new(), 0.);
                self.feast.hold(id, false);
                self.feast.set_aimed(id, false);
                continue;
            }
            let n = if seat.queue.len() > BACKLOG { 2 } else { 1 }.min(seat.queue.len()).min(seat.budget.max(0.) as usize);
            let due: Vec<Command> = seat.queue.drain(..n).collect();
            seat.budget -= n as f32;
            seat.silent = if n == 0 { seat.silent + dt } else { 0. };
            let hold = n == 0 && seat.silent <= HANG + 1e-4;
            for (k, c) in due.into_iter().enumerate() {
                if k > 0 {
                    self.feast.step_body(id, dt);
                }
                self.seats[i].applied = c.seq;
                self.command(id, &c, dt);
            }
            self.feast.hold(id, hold);
        }
    }
    /// One command of a person's, applied as single player steers its own hotdog (src/arena.rs `tick`) and as the
    /// client predicts it (src/arena.rs `predict`): the stances, the stim, the walk (`Feast::walk`, at the pace the
    /// hotdog has: crouched, prone, stimmed, laden, a quarter of it while thrown), the jump, the pitch, the gun in
    /// hand, whether it aims down its sights (`button::SCOPE`) and how it holds what it carries (`button::TURN` and
    /// `button::TIP`, while it carries something); then what was pressed: a reload (`button::RELOAD`, with its hands
    /// free), a pickup, eating, the melee
    /// strike (in the free for all, the bite), picking up or putting down, a throw, and the trigger. The simulation
    /// refuses whatever cannot be done.
    pub fn command(&mut self, id: u32, c: &Command, dt: f32) {
        let f = &mut self.feast;
        // (Not at the card table: the stances and the gun in hand are not gated there, and have no place at it.)
        if f.cards.is_some() || !f.player(id).is_some_and(|p| p.alive) {
            return;
        }
        // Kung Fu: the simulation reads the whole command its own way, as single player does (kitchen_sim::kungfu).
        if f.kungfu.is_some() {
            return f.kungfu_command(id, c, dt);
        }
        let (prone, crouch) = (c.has(button::PRONE), c.has(button::CROUCH) && !c.has(button::PRONE));
        f.set_prone(id, prone);
        f.set_crouch(id, crouch);
        if c.has(button::STIM) {
            f.stim(id);
        }
        let stim = f.player(id).is_some_and(|p| p.stim > 0.);
        let control = if f.body(id).is_some_and(|body| body.launched) { f.bodies.config.control as f32 } else { 1. };
        // (At the pace and the jump the tuning has: kitchen_sim::tune, the constants unless --tune or the sandbox's
        // panel changed them; the client predicts with the same numbers, which the server tells it.)
        let step = c.stride(f.tune.pace(prone, crouch, stim, f.burden(id)) * control, dt);
        f.walk(id, step, c.yaw());
        if c.has(button::JUMP) && !prone {
            f.jump(id, f.tune.leap(stim));
        }
        f.look(id, c.pitch());
        if (1..kitchen_sim::feast::GUNS).contains(&(c.weapon as usize)) {
            f.select(id, c.weapon as usize);
        }
        f.set_aimed(id, c.has(button::SCOPE));
        // How it holds what it carries (`button::TURN`, `button::TIP`): before a put, so that it lands as it was held.
        // With its hands free those bits are no pose: both of TIP's are R, a reload (kitchen_sim magazine.rs).
        let carrying = f.player(id).is_some_and(|p| p.carry >= 0);
        if carrying {
            f.set_held_pose(id, c.pose());
        } else if c.reloads() {
            f.reload(id);
        }
        let direction = c.direction();
        if let Some(item) = c.pickup {
            f.pickup(id, item as usize);
        }
        if c.has(button::EAT) {
            f.eat(id);
        }
        if c.has(button::MELEE) {
            f.melee(id, direction);
        }
        if c.has(button::GRAB) {
            if f.player(id).is_some_and(|p| p.carry >= 0) {
                f.put(id, direction);
            } else {
                f.grab(id, direction);
            }
        }
        if c.has(button::THROW) {
            f.toss(id, direction);
        }
        if c.has(button::FIRE) {
            let weapon = f.player(id).map_or(0, |p| p.weapon);
            f.fire(id, direction, weapon);
        }
    }

    // ----- Fully Loaded.

    /// A person's flip, stop or choice of a hotdog at the card table, for the seat they sit at (`seat`, which the
    /// server works out from the hotdog a socket plays and never takes from what the socket says). Refused, with
    /// nothing changed, when it is not a person's seat now (a bot's plays itself: so does one whose person is waiting
    /// in the fridge for the next game), when no round is being played, when there is no table, or when the table says
    /// it is not that seat's go (kitchen_sim::cards). Returns whether it was taken.
    pub fn cards(&mut self, seat: usize, command: cards::Command) -> bool {
        let (Some(player), Some(held)) = (self.feast.players.get(seat), self.seats.get(seat)) else {
            return false;
        };
        if self.phase != Phase::Round || player.bot || !held.connected || held.waiting {
            return false;
        }
        let id = player.id;
        self.feast.cards_command(id, command)
    }

    // ----- What clients are told.

    /// What every game socket is told of the game (the lobby adds its leader, its choice and the sides; the hub each
    /// client's own step, ack and body). Every loose object is in it: a frame says what changed (net.rs).
    pub fn frame(&self) -> Frame {
        let f = &self.feast;
        let players = if self.occupied {
            self.feast
                .players
                .iter()
                .zip(&self.seats)
                .map(|(player, seat)| Shown {
                    player: player.clone(),
                    color: seat.color.clone(),
                    human: seat.connected,
                    waiting: seat.waiting,
                    talk: if seat.connected { seat.talk } else { 0. },
                    profile: seat.profile.clone(),
                    wish: seat.wish,
                })
                .collect()
        } else {
            vec![]
        };
        Frame {
            kind: "snapshot".into(),
            countdown: self.countdown,
            opening: self.opening,
            opening_for: self.pause[1],
            round: f.round,
            elapsed: f.elapsed,
            stage: self.stage().into(),
            warmup: f.warmup,
            famine_at: f.famine_at,
            alert: f.alert.clone(),
            winner: f.winner,
            wave_spawned: f.wave_spawned,
            wave_kills: f.wave_kills,
            friendly_fire_events: f.friendly_fire_events,
            players,
            enemies: f.enemies.clone(),
            emergences: f.emergences.clone(),
            pickups: f.pickups.clone(),
            shots: f.shots.clone(),
            projectiles: f.projectiles.clone(),
            splats: f.splats.clone(),
            pests: f.pests.clone(),
            turrets: f.turret_yaw.clone(),
            props: (0..f.props.len())
                .map(|id| Loose { id, state: f.props.state(id) as u8, p: f.props.position(id), q: f.props.rotation(id), v: f.props.velocity(id), w: f.props.angular_velocity(id) })
                .collect(),
            knocks: f.knocks.clone(),
            drapes: f.drapes.clone(),
            forts: f.fort_state(),
            rules: f.rules.text(),
            ends: f.ends,
            ..Default::default()
        }
    }
}
/// A stage of a round, as the clients name it.
pub fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Warmup => "Warmup",
        Stage::Wave => "Wave",
        Stage::FreeForAll => "FreeForAll",
        Stage::RoundOver => "RoundOver",
        Stage::Build => "Build",
        Stage::Fight => "Fight",
    }
}


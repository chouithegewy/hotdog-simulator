// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Forts, the team mode: the rules it adds to the simulation, on any map with the Forts layout (its `zones`: two bases,
//! the field and the bacon's spot; a map made for Forts, `mode: "forts"`, has them filled in where it leaves them out:
//! map.rs `fortress`). The mode is the game's (rules.rs `Mode`): a map made for Forts is played as Forts unless the rules
//! say otherwise, and a map with the layout may be played as Forts. The server plays them with this same code. `Feast`
//! keeps a `Fort` in `fort` while the round is played as Forts (None otherwise) and calls in here at a few points of a
//! step; everything else is as in every gun mode (rules.rs: the loadout, the pickups, the strike, kills).
//!
//! Two teams of three, the Mustard Eaters (team 0) and the Ketchup Connoisseurs (team 1), each with a base at
//! its end of the map. A match is rounds; a round is:
//!   Build      the forcefield stands across the map (as a block of the level, so bodies, loose objects,
//!              rounds and routes all stop at it) for the rules' build time (map.rs BUILD, 150 s, unless the server's
//!              --build or the page's ?build= say otherwise: `set_build`; a map's own `build` is passed over). Nobody can be hurt. Each side
//!              carries its loose objects about to build a fortress. The weapon pickups are on their pads
//!              from the start, and each side can take those on its own side of the field (the guns stay
//!              locked until it drops). Once every living hotdog a person plays is ready, the field drops early.
//!   Fight      the field is down and the pickups on both sides can be had. The bacon lies at its spot: whoever touches it
//!              carries it (and can only strike, not shoot), and a carrier who stands in their own base has
//!              won the round for their team. A carrier who dies drops it where they fell; left alone for
//!              RETURN seconds it goes back to its spot. A team with nobody left alive has lost the round.
//!              Nobody comes back to life within a round. STALE seconds without a death and without the bacon
//!              being picked up and the microwave lets out zombie cats: PACK of them, then GROWTH more each
//!              time it happens again in the round (at most PACK_MAX at once). Cats go for whoever is
//!              nearest, of either team.
//!   RoundOver  the score is shown for INTERMISSION seconds; then the next round's build phase, with
//!              everything put back. The first team to ROUNDS rounds has won the match, and a new match
//!              begins (the server returns to its lobby instead).
//! There is no hunger, no eating and no pests, and team mates' rounds, bursts and blows do not touch each
//! other. Every hotdog starts a round with the loadout of every gun mode (magazine.rs `loadout`: the syrup rifle in hand
//! and the mustard pistol beside it, full magazines and their spare rounds). While the forcefield stands nobody can fire,
//! strike or reload (a carried object can still be thrown: that is building; R is Ready), and each side takes only the
//! pickups on its own side of it. The pickups, the power weapons among them, are as in every gun mode (magazine.rs).

use crate::actor;
use crate::feast::{chest, muzzle, Feast, Player, Stage, MAYO, RIFLE};
use crate::map::{Block, Level, FLOOR, FORT_PLAYERS, MARGIN, TEAM_NAMES};
use crate::props::PropLevel;
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// What the bots build in the build phase, and how (blueprints of forts, fitted to the map and built piece by piece).
#[path = "blueprint.rs"]
pub mod blueprint;

pub const ROUNDS: u32 = 3;
pub const INTERMISSION: f32 = 8.;
pub const RETURN: f32 = 6.;
pub const STALE: f32 = 45.;
pub const PACK: u32 = 2;
pub const GROWTH: u32 = 2;
pub const PACK_MAX: u32 = 12;
pub const TOUCH: f32 = 0.12;
/// The hotdogs' names, by team: nobody on the ketchup side is called MUSTARD.
pub const NAMES: [[&str; 3]; 2] = [["DIJON", "HONEY", "GRAINY"], ["TOMATO", "CATSUP", "PASSATA"]];
/// Where the bacon is: on its spot, in somebody's arms, or lying where its carrier fell.
pub const HOME: u8 = 0;
pub const HELD: u8 = 1;
pub const DROPPED: u8 = 2;
/// A pickup's `ready` for one that is not to be had until later. (Forts no longer holds its pickups back: they are
/// on their pads from the start of the build phase. The value stays, for whatever reads it.)
pub const LATER: f32 = 1e9;
/// The bots build their team's fort to a blueprint (blueprint.rs). In the fight the one that stands guard stands
/// inside it, behind the wall by the gate; on a team with no plan, GUARD metres in front of its base. A bot that has got
/// nowhere for a second jumps and steps aside for DODGE seconds.
pub const GUARD: f32 = 0.55;
pub const DODGE: f32 = 0.7;
/// The share of its pace the bacon's carrier keeps (`Feast::burden`): it is carried home slower than the hotdogs walk,
/// slower than a crate (feast.rs SLOW), so that the other side has time to stop it. It was 0.75; since the bots reach the
/// bacon however the pieces have come to rest round it (`soldier`) and every gun mode's bots fire as soon as they are
/// aimed (no more bursts), at 0.75 nearly every round of a bot match was a capture some 7 s after the field dropped; at
/// 0.55 about half are, and the rest are wipes (aim_tests.rs `measure_forts`).
pub const BACON_PACE: f32 = 0.55;
/// The last of a team facing more than one rival keeps to its base until the fight has been quiet for LONE seconds.
pub const LONE: f32 = 20.;
/// A bot carrying the bacon takes its stim when a rival or a cat is within CHASED metres.
pub const CHASED: f32 = 1.2;
/// The height of the forcefield as a block: the greatest a map's block may have (map.rs).
const FIELD_HEIGHT: f64 = 3.;

/// The bacon: `state` is HOME, HELD or DROPPED, `holder` the hotdog carrying it (0: nobody), `p` where it is
/// and `idle` the seconds it has lain dropped.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BaconState {
    pub state: u8,
    pub holder: u32,
    pub p: [f32; 3],
    pub idle: f32,
}
/// What a snapshot says of the mode (the server's `forts`): the teams' scores, the rounds a match needs, the
/// match's number, the seconds a build phase lasts and when this one ends (on the round's clock), whether the
/// field is up, the bacon, seconds until the next stalemate alarm and how many there have been, and how the
/// last round ended (`winner` is a team, or -1 for nobody; `reason` "capture" or "wipe"; `champion` the team
/// that has won the match).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FortState {
    pub scores: [u32; 2],
    pub rounds: u32,
    #[serde(rename = "match")]
    pub match_no: u32,
    pub build: f32,
    pub ends: f32,
    pub field: bool,
    pub bacon: BaconState,
    pub stale: f32,
    pub alarms: u32,
    pub winner: Option<i32>,
    pub reason: String,
    pub champion: Option<u32>,
}
/// The mode's state: the match (scores, its number, the champion) and the round (when the build phase ends,
/// whether the field is up, the bacon, the stalemate clock, how it ended).
pub struct Fort {
    pub scores: [u32; 2],
    pub match_no: u32,
    /// Seconds a build phase lasts, and when this round's ends on the round's clock.
    pub build: f32,
    pub ends: f32,
    pub up: bool,
    pub bacon: BaconState,
    /// The round's clock when something last happened (a death, the bacon taken), and the alarms so far.
    pub quiet: f32,
    /// Seconds a fight may go without a death or the bacon being taken before the microwave goes off (STALE unless
    /// the server's --stale says otherwise).
    pub stale: f32,
    pub alarms: u32,
    pub winner: Option<i32>,
    pub reason: String,
    pub champion: Option<u32>,
    /// The level without the forcefield, for when it drops, and with it; and the field as a block of the level
    /// (it stands last, after any drape's wall: drape.rs).
    pub(crate) open: Level,
    pub(crate) walled: Level,
    pub(crate) field: Block,
    /// Each team's fort as its bots are to build it this round (blueprint.rs), and whether it has been drawn yet (it
    /// is when the team's bots first look for it, in the build phase).
    pub plans: [Option<blueprint::Plan>; 2],
    pub(crate) drawn: [bool; 2],
    /// The level the bots find their way on (`Feast::renav`): the level as it stands with every piece standing on the
    /// counter as a block too, what it was made from, and when that was last looked at.
    pub(crate) nav: Option<Level>,
    pub(crate) nav_key: Vec<i32>,
    pub(crate) nav_at: f32,
}
/// A Forts bot's work (blueprint.rs): the loose object it is fetching or carrying and the place of its team's plan it
/// is for (-1: none), when it set out, the objects it has given up on, and its watch on its own progress (where it was
/// at the last check, when that was, until when it steps aside, and which way). Carrying: whether it has worked out
/// how to put it down (`stood`), where it stands for that, the way it faces and how it holds it, how many times it has
/// stepped over to put it better, whether it is only setting it down out of the way (`aside`), when it came within a
/// few centimetres of where it stands (-1: it has not), the nearest it has got there and when it last got nearer, and
/// the ways round (of four, a bit each) it has found it cannot get to; and since when it has been giving up on the
/// objects in `shun` (after a while it tries them again); and the object it was after when it picked up another
/// instead (-1: none), which it sets this one down well away from; and where it has found that setting what it carries
/// down out of the way would not do (a napkin that would drape over what lies there).
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub prop: i32,
    pub slot: i32,
    pub since: f32,
    pub shun: Vec<usize>,
    pub at: [f32; 2],
    pub checked: f32,
    pub dodge: f32,
    pub turn: f32,
    pub stood: bool,
    pub stand: [f32; 2],
    pub facing: f32,
    pub pose: crate::feast::build::Pose,
    pub tries: u8,
    pub aside: bool,
    pub near: f32,
    pub best: f32,
    pub gain: f32,
    pub banned: u8,
    pub shunned: f32,
    pub missed: i32,
    pub avoid: Vec<[f32; 2]>,
}
impl Default for Job {
    fn default() -> Job {
        Job { prop: -1, slot: -1, since: 0., shun: vec![], at: [0.; 2], checked: 0., dodge: 0., turn: 1., stood: false, stand: [0.; 2], facing: 0., pose: Default::default(), tries: 0, aside: false, near: -1., best: f32::INFINITY, gain: 0., banned: 0, shunned: 0., missed: -1, avoid: vec![] }
    }
}
/// A level as a round's loose objects are put out on it: with the second field set (a Forts round's forcefield up), the
/// field stands as one more block, the last, as it does in the level the bodies walk (`Level::fielded`, whose `map`
/// does not list it). `PropWorld::unblock` takes it away when the field drops.
pub(crate) struct Fielded<'a>(pub &'a Level, pub bool);
impl PropLevel for Fielded<'_> {
    fn ground(&self, x: f64, z: f64) -> f64 {
        self.0.ground(x, z)
    }
    fn bounds(&self) -> [f64; 4] {
        PropLevel::bounds(self.0)
    }
    fn blocks(&self) -> Vec<[f64; 5]> {
        let mut blocks = PropLevel::blocks(self.0);
        if let Some(fort) = self.0.fort.as_ref().filter(|_| self.1) {
            blocks.push([fort.field[0], fort.field[1], fort.field[2], fort.field[3], FLOOR + FIELD_HEIGHT]);
        }
        blocks
    }
}
fn clock(t: f32) -> String {
    let s = t.ceil().max(0.) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}
/// The way a hotdog looks, from its yaw and pitch.
fn gaze(p: &Player) -> Vec3 {
    Vec3::new(-p.yaw.sin() * p.pitch.cos(), p.pitch.sin(), -p.yaw.cos() * p.pitch.cos())
}

impl Feast {
    /// A round is set out as Forts (for every new round of a map made for Forts, and by `set_rules` where the rules ask
    /// for Forts; a map without the Forts layout is left alone): the level is the one with the forcefield standing in
    /// it, six hotdogs are on their teams (the odd-numbered are the Mustard Eaters) at their teams' starts with the
    /// loadout of every gun mode, the bacon is on its spot, the weapon pickups are on their pads, and the build phase
    /// runs for the rules' build time (`set_build`).
    pub(crate) fn fortify(&mut self) {
        if self.level.fort.is_none() {
            return;
        }
        let (build, stale) = (self.fort_build, self.fort_stale);
        let map = self.level.map.clone();
        let open = Level::new(map.clone());
        self.level = Level::fielded(map);
        let (level, demo) = (&self.level, self.demo);
        self.players = (0..FORT_PLAYERS)
            .map(|i| {
                let (team, rank) = (i % 2, i / 2);
                let (p, yaw) = level.team_start(team, rank);
                let p = p.map(|v| v as f32);
                let mut player = Player {
                    id: i as u32 + 1,
                    name: NAMES[team][rank % 3].into(),
                    p,
                    yaw: yaw as f32,
                    hp: 100.,
                    alive: true,
                    bot: demo || i > 0,
                    camp: [p[0], p[2]],
                    grounded: true,
                    carry: -1,
                    team: team as u8,
                    job: Job { at: [p[0], p[2]], ..Default::default() },
                    ..Default::default()
                };
                crate::magazine::loadout(&mut player);
                player
            })
            .collect();
        self.stage = Stage::Build;
        let bacon = BaconState { state: HOME, holder: 0, p: self.level.bacon_position().map(|v| v as f32), idle: 0. };
        let (walled, field) = (self.level.clone(), Block { rect: self.level.fort.as_ref().map_or([0.; 4], |fort| fort.field), height: FIELD_HEIGHT, style: "steel".into(), facing: None });
        self.fort = Some(Fort { scores: [0, 0], match_no: 1, build, ends: self.elapsed + build, up: true, bacon, quiet: 0., stale, alarms: 0, winner: None, reason: String::new(), champion: None, open, walled, field, plans: [None, None], drawn: [false; 2], nav: None, nav_key: vec![], nav_at: 0. });
        // The pickups are there from the start of the build phase: each side takes those on its own side of the
        // field while it stands (`Feast::within_reach`), and fires them once it drops.
        for item in &mut self.pickups {
            item.ready = 0.;
        }
        self.bodies.extra = self.extra_walls();
        // (The loose objects are put out afresh, with the field as their last block.)
        self.props = crate::feast::furnish(&self.level, &self.bodies.config, true);
        self.herald();
    }
    /// A new round of the same match: the scores, the match's number and the build time go on from `old`.
    pub(crate) fn inherit(&mut self, old: Option<Fort>) {
        if let (Some(old), Some(fort)) = (old, self.fort.as_mut()) {
            (fort.scores, fort.match_no, fort.champion) = (old.scores, old.match_no, old.champion);
            (fort.build, fort.ends, fort.stale) = (old.build, old.build, old.stale);
            self.herald();
        }
    }
    /// Replaces the seconds a build phase lasts (the rules' build time: map.rs BUILD unless the server's --build or the
    /// page's ?build= say otherwise), for every Forts round from now on, and the end of the one under way.
    pub fn set_build(&mut self, seconds: f32) {
        self.fort_build = seconds;
        let building = self.stage == Stage::Build;
        if let Some(fort) = &mut self.fort {
            fort.build = seconds;
            if building {
                fort.ends = seconds;
                self.herald();
            }
        }
    }
    /// Replaces the seconds a fight may stay quiet before the microwave goes off (the server's --stale), for every Forts
    /// round from now on.
    pub fn set_stale(&mut self, seconds: f32) {
        self.fort_stale = seconds;
        if let Some(fort) = &mut self.fort {
            fort.stale = seconds;
        }
    }
    /// What a snapshot says of the mode; None on any other map.
    pub fn fort_state(&self) -> Option<FortState> {
        let fort = self.fort.as_ref()?;
        Some(FortState {
            scores: fort.scores,
            rounds: ROUNDS,
            match_no: fort.match_no,
            build: fort.build,
            ends: fort.ends,
            field: fort.up,
            bacon: fort.bacon.clone(),
            stale: if self.stage == Stage::Fight { (fort.stale - (self.elapsed - fort.quiet)).max(0.) } else { fort.stale },
            alarms: fort.alarms,
            winner: fort.winner,
            reason: fort.reason.clone(),
            champion: fort.champion,
        })
    }
    /// Forts: whether two hotdogs are on one team, whose rounds, bursts and blows spare each other.
    pub fn allied(&self, a: u32, b: u32) -> bool {
        // (Titan: the small hotdogs are one team, the titan the other: titan.rs.)
        (self.fort.is_some() || self.titan.is_some()) && matches!((self.player(a), self.player(b)), (Some(a), Some(b)) if a.team == b.team)
    }
    /// The people playing who are still alive, and how many of them are ready: when all of them are, the
    /// build phase ends early.
    fn people(&self) -> (usize, usize) {
        let people = self.players.iter().filter(|p| p.alive && !p.bot);
        (people.clone().count(), people.filter(|p| p.ready).count())
    }
    /// A person says whether they are ready for the build phase to end (only the build phase listens, and
    /// only to the living). Returns whether it was taken.
    pub fn ready(&mut self, id: u32, flag: bool) -> bool {
        if self.fort.is_none() || self.stage != Stage::Build {
            return false;
        }
        match self.player_mut(id).filter(|p| p.alive && !p.bot) {
            Some(p) => {
                p.ready = flag;
                true
            }
            None => false,
        }
    }
    /// The build phase's line at the top of the screen: the time left, and how many of the people playing are ready.
    pub fn herald(&mut self) {
        let Some(ends) = self.fort.as_ref().map(|fort| fort.ends) else {
            return;
        };
        let (people, ready) = self.people();
        let asked = if people > 0 { format!(" {ready} OF {people} READY.") } else { String::new() };
        self.alert = format!("BUILD YOUR BASE. THE FORCEFIELD DROPS IN {}.{asked}", clock(ends - self.elapsed));
    }
    /// The forcefield comes down: the level is the open one again and the loose objects lose their wall.
    /// (The network client calls this when the server's field has dropped; the stage and the pickups are the
    /// server's to say.) Nothing happens when it is not up.
    pub fn lower_field(&mut self) {
        let Some(fort) = self.fort.as_mut().filter(|fort| fort.up) else {
            return;
        };
        fort.up = false;
        self.relevel();
        self.props.unblock();
    }
    /// The forcefield drops: the guns are unlocked, both sides' pickups can be had and the fight is on. (A pickup
    /// taken during the build is still on its way back: its time is not cut short.)
    fn drop_field(&mut self) {
        self.lower_field();
        self.stage = Stage::Fight;
        if let Some(fort) = &mut self.fort {
            fort.quiet = self.elapsed;
        }
        for p in &mut self.players {
            p.ready = false;
        }
        self.alert = "THE FORCEFIELD IS DOWN. STEAL THE BACON AND BRING IT HOME.".into();
    }
    /// One step of the rules, before the bodies move.
    pub(crate) fn rule(&mut self, dt: f32) {
        let Some(fort) = &self.fort else {
            return;
        };
        match self.stage {
            Stage::Build => {
                let (people, ready) = self.people();
                if self.elapsed >= fort.ends || (people > 0 && ready == people) {
                    self.drop_field();
                } else {
                    self.herald();
                }
                return;
            }
            Stage::Fight => {}
            _ => return,
        }
        let (mut b, home) = (fort.bacon.clone(), self.level.bacon_position().map(|v| v as f32));
        let restore = |b: &mut BaconState| *b = BaconState { state: HOME, holder: 0, p: home, idle: 0. };
        // The bacon goes with its carrier, who wins the round by standing in their own base with it; a dead
        // carrier drops it where they fell.
        if b.holder != 0 {
            match self.player(b.holder).filter(|h| h.alive).map(|h| (h.p, h.team)) {
                Some((p, team)) => {
                    b.p = p;
                    if self.level.in_base(team as usize, p[0] as f64, p[2] as f64) {
                        let carrier = b.holder;
                        self.fort.as_mut().unwrap().bacon = b;
                        return self.win(team as i32, "capture", Some(carrier));
                    }
                }
                None => {
                    let fell = self.player(b.holder).map(|h| (h.p[0] as f64, h.p[2] as f64)).filter(|(x, z)| self.level.walkable(*x, self.level.ground(*x, *z), *z));
                    (b.holder, b.state, b.idle) = (0, DROPPED, 0.);
                    self.alert = "THE BACON HAS BEEN DROPPED.".into();
                    match fell {
                        Some((x, z)) => b.p = [x as f32, self.level.ground(x, z) as f32, z as f32],
                        None => restore(&mut b),
                    }
                }
            }
        }
        // Lying free, it is taken by the nearest living hotdog that touches it, who lets go of whatever else
        // they carried; dropped and left alone, it goes back to its spot.
        if b.holder == 0 {
            let (mut taker, mut least) = (None, TOUCH);
            for p in self.players.iter().filter(|p| p.alive && (p.p[1] - b.p[1]).abs() < 0.12) {
                let far = (p.p[0] - b.p[0]).hypot(p.p[2] - b.p[2]);
                if far < least {
                    (least, taker) = (far, Some(p.id));
                }
            }
            if let Some(id) = taker {
                if let Some(d) = self.player(id).filter(|p| p.carry >= 0).map(gaze) {
                    self.put(id, d);
                }
                let p = self.player_mut(id).unwrap();
                p.weapon = 0;
                (b.holder, b.state, b.idle, b.p) = (id, HELD, 0., p.p);
                self.alert = format!("{} OF THE {} HAS THE BACON.", p.name, TEAM_NAMES[p.team as usize].to_uppercase());
                self.fort.as_mut().unwrap().quiet = self.elapsed;
            } else if b.state == DROPPED {
                b.idle += dt;
                if b.idle >= RETURN {
                    restore(&mut b);
                    self.alert = "THE BACON IS BACK IN THE MIDDLE.".into();
                }
            }
        }
        self.fort.as_mut().unwrap().bacon = b;
        // A team with nobody left has lost the round (both at once: nobody has won it).
        let left = [0, 1].map(|t| self.players.iter().filter(|p| p.team == t && p.alive).count());
        if left[0] == 0 || left[1] == 0 {
            return self.win(if left[0] > 0 { 0 } else if left[1] > 0 { 1 } else { -1 }, "wipe", None);
        }
        // Nothing has happened for too long: the microwave goes off, with more cats each time.
        let fort = self.fort.as_mut().unwrap();
        if self.elapsed - fort.quiet >= fort.stale {
            let cats = (PACK + GROWTH * fort.alarms).min(PACK_MAX);
            fort.alarms += 1;
            fort.quiet = self.elapsed;
            self.emerge(cats);
            self.alert = format!("STALEMATE. THE MICROWAVE DINGS: {cats} ZOMBIE CATS.");
        }
    }
    /// The round is over: `team` has won it (-1: nobody), by a "capture" or a "wipe". Rounds still in the air
    /// can no longer hurt anybody. (Public for the server's tests, which end rounds with it.)
    pub fn win(&mut self, team: i32, reason: &str, carrier: Option<u32>) {
        self.stage = Stage::RoundOver;
        self.over_at = self.elapsed;
        for q in &mut self.projectiles {
            q.hostile = false;
        }
        self.winner = carrier;
        let hero = carrier.and_then(|id| self.player(id)).map(|p| p.name.clone()).unwrap_or_default();
        let Some(fort) = &mut self.fort else {
            return;
        };
        (fort.winner, fort.reason) = (Some(team), reason.into());
        let name = if team >= 0 {
            fort.scores[team as usize] += 1;
            TEAM_NAMES[team as usize].to_uppercase()
        } else {
            String::new()
        };
        let [a, b] = fort.scores;
        fort.champion = (team >= 0 && fort.scores[team as usize] >= ROUNDS).then_some(team as u32);
        self.alert = if fort.champion.is_some() {
            format!("THE {name} WIN THE MATCH, {} ROUNDS TO {}.", a.max(b), a.min(b))
        } else if team < 0 {
            format!("NOBODY IS LEFT STANDING. NO POINT. MUSTARD {a}, KETCHUP {b}.")
        } else if reason == "capture" {
            format!("{hero} BROUGHT HOME THE BACON. MUSTARD {a}, KETCHUP {b}.")
        } else {
            format!("THE {name} ARE THE LAST STANDING. MUSTARD {a}, KETCHUP {b}.")
        };
    }
    /// The pause after a round: then the next round, or after the match a new match.
    pub(crate) fn rest(&mut self, dt: f32) {
        if self.elapsed - self.over_at <= INTERMISSION {
            return self.settle(dt);
        }
        // The match is over: the next one starts from nothing.
        if let Some(fort) = self.fort.as_mut().filter(|fort| fort.champion.is_some()) {
            fort.match_no += 1;
            (fort.scores, fort.champion) = ([0, 0], None);
        }
        self.reset();
    }

    // ----- Bots.

    /// The middle of a team's base, and the way from it to the forcefield, both [x, z].
    fn heart(&self, t: usize) -> [f32; 2] {
        let r = self.level.fort.as_ref().map_or([0.; 4], |fort| fort.bases[t]);
        [((r[0] + r[2]) / 2.) as f32, ((r[1] + r[3]) / 2.) as f32]
    }
    fn front(&self, t: usize) -> [f32; 2] {
        match &self.level.fort {
            Some(fort) if fort.axis == 1 => [0., -fort.sides[t] as f32],
            Some(fort) => [-fort.sides[t] as f32, 0.],
            None => [0.; 2],
        }
    }
    /// A point `ahead` metres in front of a team's base, `across` to its left as it looks at the field.
    pub(crate) fn before(&self, t: usize, ahead: f32, across: f32) -> [f32; 2] {
        let (c, u) = (self.heart(t), self.front(t));
        let out = self.level.fort.as_ref().map_or(0., |fort| {
            let r = fort.bases[t];
            (if fort.axis == 1 { r[3] - r[1] } else { r[2] - r[0] }) / 2.
        }) as f32
            + ahead;
        [c[0] + u[0] * out - u[1] * across, c[1] + u[1] * out + u[0] * across]
    }
    /// Where a team's guard stands in the fight: inside its fort behind the wall by the gate (blueprint.rs `Plan::posts`),
    /// or GUARD metres in front of its base.
    pub(crate) fn post(&self, t: usize) -> [f32; 2] {
        match self.plan_of(t).and_then(|plan| plan.posts.first()) {
            Some(p) => [p[0] as f32, p[1] as f32],
            None => self.before(t, GUARD, 0.),
        }
    }
    /// A bot walks to [x, z], round the pieces standing on the counter (`Feast::wend`): into and out of a fort by its
    /// gate. One that has got nowhere in a second (a wall of crates in its way, say) jumps and goes sideways for a
    /// moment, the other way each time.
    fn march(&mut self, id: u32, to: [f32; 2], dt: f32) {
        self.wend(id, Vec3::new(to[0], self.level.ground_at(to[0], to[1]), to[1]), dt);
        let elapsed = self.elapsed;
        let Some(p) = self.player_mut(id) else {
            return;
        };
        let (at, j) = (p.p, &mut p.job);
        let mut jump = false;
        if elapsed - j.checked >= 1. {
            if (at[0] - j.at[0]).hypot(at[2] - j.at[1]) < 0.03 {
                (j.dodge, j.turn, jump) = (elapsed + DODGE, -j.turn, true);
            }
            (j.at, j.checked) = ([at[0], at[2]], elapsed);
        }
        let (aside, turn) = (elapsed < j.dodge, j.turn as f64);
        if let Some(body) = self.bodies.get_mut(id) {
            if jump {
                body.jump = actor::JUMP;
            }
            if aside {
                body.wish = [-body.wish[1] * turn, body.wish[0] * turn];
            }
        }
    }
    /// Whether a glob flying from `o` to `c` would burst clear of the bot's own team (see `clear_of`).
    fn safe(&self, id: u32, o: Vec3, c: Vec3) -> bool {
        let team = self.player(id).map_or(0, |p| p.team);
        let room = 0.35 + actor::BOT_WALK as f32 * o.distance(c) / self.tune.speed[4];
        let (x, z) = (c.x - o.x, c.z - o.z);
        let l = if x * x + z * z > 0. { x * x + z * z } else { 1. };
        self.players.iter().filter(|q| q.id != id && q.alive && q.team == team).all(|q| {
            let t = (((q.p[0] - o.x) * x + (q.p[2] - o.z) * z) / l).clamp(0., 1.);
            (q.p[0] - o.x - x * t).hypot(q.p[2] - o.z - z * t) >= room
        })
    }
    pub(crate) fn fort_bot(&mut self, id: u32, dt: f32) {
        self.renav();
        match self.stage {
            Stage::Build => self.mason(id, dt),
            Stage::Fight => self.soldier(id, dt),
            _ => {}
        }
    }
    /// The fight: a bot with the bacon runs home with it and does nothing else. Otherwise it puts
    /// down whatever it carried, fetches a gun when it has none loaded (or one is near and nobody is), and
    /// shoots the nearest rival or cat it can see with the gun that suits the range, choosing and trading guns as in
    /// every gun mode (`Feast::arm`, `swap`), and firing whenever it is aimed and ready. Two of a
    /// team's three make for the bacon, or for whoever carries it (to guard a team mate, to stop a rival); the
    /// third, while all three live, stands guard inside its fort, behind the wall by the gate (in front of its base
    /// on a team with no plan; which of them it is changes with the round). Into and out of a fort it goes by the
    /// gate (`march`). One that draped walls have cut off from where it is going breaks out (`breach`); one low on
    /// rounds goes further for a pickup.
    fn soldier(&mut self, id: u32, dt: f32) {
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let (t, at, b) = (p.team, Vec3::from_array(p.p), self.fort.as_ref().unwrap().bacon.clone());
        let mine = b.holder == id;
        // Whatever it still carries it puts down where it stands; in its base, where nothing may be put, it first walks
        // out of the base by its fort's gate.
        if p.carry >= 0 && !self.put(id, gaze(&p)) {
            let way = self.plan_of(t as usize).map_or_else(|| self.before(t as usize, GUARD, 0.), |plan| [plan.gate[0] as f32, plan.gate[1] as f32]);
            return self.march(id, way, dt);
        }
        // The nearest rival or formed cat: its chest, how it moves, whether it is a cat, and what the bot's sight knows it
        // by (a rival's id; CAT and a cat's).
        let (mut foe, mut least) = (None, f32::INFINITY);
        for q in self.players.iter().filter(|q| q.alive && q.team != t) {
            let far = Vec3::from_array(q.p).distance(at);
            if far < least {
                (least, foe) = (far, Some((chest(q), Vec3::from_array(q.v), false, q.id)));
            }
        }
        for e in self.enemies.iter().filter(|e| e.forming <= 0.) {
            let far = Vec3::from_array(e.p).distance(at);
            if far < least {
                (least, foe) = (far, Some((Vec3::from_array(e.p) + Vec3::Y * 0.075, Vec3::new(-e.yaw.sin(), 0., -e.yaw.cos()) * actor::ZOMBIE_WALK as f32, true, crate::aim::cat(e.id))));
            }
        }
        let holder = self.player(b.holder).filter(|_| b.holder != 0).map(|h| (h.p, h.team));
        let loaded = [1, 2, 3, 4, RIFLE, MAYO].into_iter().any(|w| p.owned[w] && crate::magazine::rounds(&p, w) > 0);
        // While a team has more than one alive, the one with the least HP left (at first, a different one each
        // round) stands guard in front of its base, and leaves it only to stop a rival who has the bacon. The
        // last of a team stands guard too while it is outnumbered, until nothing has happened for LONE seconds.
        let turn = |q: &Player| (q.id + self.round) % 6;
        let mates = self.players.iter().filter(|q| q.alive && q.team == t).count();
        let rivals = self.players.iter().filter(|q| q.alive && q.team != t).count();
        let ahead = self.players.iter().filter(|q| q.alive && q.team == t && (q.hp > p.hp || (q.hp == p.hp && turn(q) < turn(&p)))).count();
        let guard = if mates > 1 { ahead == mates - 1 } else { rivals > 1 && self.elapsed - self.fort.as_ref().unwrap().quiet < LONE };
        // With the bacon it only runs for home, on a stim when anything is after it.
        if mine {
            if foe.is_some() && least < CHASED {
                self.stim(id);
            }
            self.march(id, self.heart(t as usize), dt);
            return;
        }
        // Low on rounds it goes further for a pickup, a box of ammunition above all (see `wants` and `low`).
        let reach = if !loaded {
            3.
        } else if Self::low(&p) && least > 0.6 {
            1.6
        } else if least > 1. {
            0.8
        } else {
            0.
        };
        if !self.errand(id, reach, dt) {
            let goal = match holder {
                _ if guard && !holder.is_some_and(|(_, team)| team != t) => self.post(t as usize),
                Some((h, _)) => [h[0], h[2]],
                None => [b.p[0], b.p[2]],
            };
            let far = (goal[0] - p.p[0]).hypot(goal[1] - p.p[2]);
            // (Pieces knocked about in a fight may come to rest round the bacon where it lies, so that the way round the
            // pieces finds no way in to it: near it, a bot then walks straight at it, shoving whatever is in the way.)
            let lying = holder.is_none() && goal == [b.p[0], b.p[2]] && far < 0.8;
            let shut = lying && self.fort.as_ref().and_then(|fort| fort.nav.as_ref()).is_some_and(|nav| !nav.joined([p.p[0] as f64, p.p[2] as f64], [goal[0] as f64, goal[1] as f64]));
            if shut {
                self.advance_actor(id, Vec3::new(goal[0], b.p[1], goal[1]), dt);
            } else if !self.breach(id, goal, dt) && far > if holder.is_some_and(|(_, team)| team == t) { 0.3 } else { 0.05 } {
                self.march(id, goal, dt);
            }
        }
        let Some((c, v, cat, key)) = foe else {
            return;
        };
        if least < 0.17 {
            let d = self.aim(id, c, Vec3::ZERO, 0.);
            self.face(id, d);
            self.melee(id, d);
            return;
        }
        // (The errand may have put a new gun in its hands.)
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let gun = self.arm(&p, least, least > 0.6 && self.safe(id, muzzle(&p), c));
        if gun == 0 {
            return;
        }
        if let Some(p) = self.player_mut(id) {
            (p.weapon, p.gun) = (gun, gun);
        }
        // The mayonnaise is lobbed, over cover if need be; the rest it fires where it has a clear shot, aiming at a rival
        // as at a cat (a relish glob at a rival's feet).
        let (d, ready) = if gun == MAYO {
            let Some(shot) = self.draw_lob(id, key, c, v) else {
                return;
            };
            shot
        } else {
            let o = muzzle(&p);
            if !(self.cover(o, self.aim(id, c, v, self.tune.speed[gun])) + 0.02 >= least && least <= self.tune.range[gun] * 0.95 && !(gun == 3 && least >= 0.6)) {
                return;
            }
            let (mark, face) = match self.player(key).filter(|_| !cat).cloned() {
                Some(rival) => self.mark(o, &rival, gun),
                None => (c, None),
            };
            self.draw_bead(id, key, mark, v, gun, face)
        };
        self.loose(id, d, ready, gun);
    }
    /// A bot in the fight whose way on foot to `goal` ([x, z]) draped walls have cut off (there is none round them,
    /// though there would be without them) breaks out: it walks up to the nearest wall, pulls the napkin off it and
    /// lays the napkin down behind itself, away from the wall, so that it does not drape there again. Returns
    /// whether it spent this step on that.
    fn breach(&mut self, id: u32, goal: [f32; 2], dt: f32) -> bool {
        let Some(at) = self.player(id).map(|p| p.p.map(|v| v as f64)) else {
            return false;
        };
        let goal = [goal[0] as f64, goal[1] as f64];
        if self.drapes.is_empty() || self.level.joined([at[0], at[2]], goal) || !self.fort.as_ref().is_some_and(|fort| fort.open.joined([at[0], at[2]], goal)) {
            return false;
        }
        let (mut wall, mut least) = (0, f64::INFINITY);
        for d in &self.drapes {
            let far = (d.rect[0] - at[0]).max(0.).max(at[0] - d.rect[2]).hypot((d.rect[1] - at[2]).max(0.).max(at[2] - d.rect[3]));
            if far < least {
                (least, wall) = (far, d.id);
            }
        }
        let Some(d) = self.drapes.iter().find(|d| d.id == wall).cloned() else {
            return false;
        };
        let near = [d.rect[0].max(d.rect[2].min(at[0])), d.rect[1].max(d.rect[3].min(at[2]))];
        let (dx, dz) = (at[0] - near[0], at[2] - near[1]);
        let l = if dx.hypot(dz) > 0. { dx.hypot(dz) } else { 1. };
        if least < MARGIN + 0.06 && at[1] < d.top {
            let yaw = (-dx).atan2(-dz) as f32;
            if let Some(p) = self.player_mut(id) {
                p.yaw = yaw;
            }
            // (Where that would be in a base, where nothing may be put, it lays it to one side or the other; failing
            // both, it carries it out by its fort's gate and puts it down there: see `soldier`.)
            if self.pull(id, wall) {
                for turn in [0., std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2] {
                    if let Some(p) = self.player_mut(id) {
                        p.yaw = yaw + turn;
                    }
                    if self.put(id, Vec3::new(-(yaw + turn).sin(), 0., -(yaw + turn).cos())) {
                        break;
                    }
                }
            }
            return true;
        }
        let out = MARGIN + 0.03;
        self.march(id, [(near[0] + dx / l * out) as f32, (near[1] + dz / l * out) as f32], dt);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magazine::{CARRY, POWER_BACK};
    use crate::feast::{self, HEAVY, KEEP, LIFT, MELEE, OUT, PLAYER_DAMAGE, RIFLE, SLOW, THROW};
    use crate::map::Map;
    use glam::Vec2;
    use crate::props::State;
    use std::collections::HashSet;

    const F: f32 = feast::FLOOR;
    /// The middle deck's top, where the bacon lies.
    const DECK: f32 = 1.0225;
    const DT: f32 = 1. / 30.;
    // The map's loose objects, in file order: the mustard side's three chairs, four crates, five pillows and
    // three napkins, then the ketchup side's.
    const CRATE: usize = 3;
    const PILLOW: usize = 7;
    const KETCHUP: usize = 15;
    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|error| panic!("{path}: {error}"))
    }
    fn map() -> Map {
        Map::parse(&read("tools/network/fixtures/long-fort.json")).unwrap()
    }
    fn hush(s: &mut Feast) {
        for p in &mut s.players {
            p.bot = false;
        }
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 30.).round() as u32 {
            s.tick(DT);
        }
    }
    /// A round with nobody doing anything: every bot is switched off (each new round switches them on again).
    fn fort(build: f32) -> Feast {
        let mut s = Feast::with_map(map(), false);
        s.set_build(build);
        hush(&mut s);
        s
    }
    /// The same once the forcefield has dropped.
    fn fight() -> Feast {
        let mut s = fort(5.);
        run(&mut s, 5.1);
        assert_eq!(s.stage, Stage::Fight);
        s
    }
    fn who(s: &Feast, id: u32) -> &Player {
        s.player(id).unwrap()
    }
    fn at(s: &mut Feast, id: u32, x: f32, z: f32) {
        s.player_mut(id).unwrap().p = [x, F, z];
    }
    /// On the middle deck, where the bacon lies.
    fn up(s: &mut Feast, id: u32, x: f32, z: f32) {
        s.player_mut(id).unwrap().p = [x, DECK, z];
    }
    fn look(s: &mut Feast, id: u32, d: [f32; 3]) -> Vec3 {
        let d = Vec3::from_array(d).normalize();
        let p = s.player_mut(id).unwrap();
        (p.yaw, p.pitch) = ((-d.x).atan2(-d.z), d.y.asin());
        d
    }
    fn land(s: &mut Feast) -> u32 {
        let mut steps = 0;
        while !s.projectiles.is_empty() && steps < 600 {
            s.tick(DT);
            steps += 1;
        }
        steps
    }
    fn pos(s: &Feast, prop: usize) -> [f32; 3] {
        s.props.position(prop).map(|v| v as f32)
    }
    fn pose(s: &mut Feast, prop: usize, p: [f32; 3]) {
        let turn = s.props.rotation(prop);
        s.props.set_pose(prop, p.map(|v| v as f64), turn);
    }
    fn walk(s: &mut Feast, id: u32, to: [f32; 2], steps: u32) {
        for _ in 0..steps {
            s.advance_actor(id, Vec3::new(to[0], F, to[1]), DT);
            s.tick(DT);
        }
    }
    fn team(s: &Feast, t: u8) -> Vec<u32> {
        s.players.iter().filter(|p| p.team == t).map(|p| p.id).collect()
    }
    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }
    fn bacon(s: &Feast) -> &BaconState {
        &s.fort.as_ref().unwrap().bacon
    }

    #[test]
    fn a_forts_map_is_played_three_a_side_six_hotdogs_the_odd_ones_mustard_eaters_each_at_one_of_its_teams_starts() {
        let s = fort(5.);
        assert_eq!(s.players.len(), 6);
        assert_eq!(s.players.iter().map(|p| p.team).collect::<Vec<_>>(), [0, 1, 0, 1, 0, 1]);
        assert_eq!(s.stage, Stage::Build);
        let sides = s.level.fort.as_ref().unwrap().sides;
        for p in &s.players {
            assert_eq!(s.level.side(p.p[0] as f64, p.p[2] as f64), sides[p.team as usize], "{} starts on its side", p.name);
            // The loadout of every gun mode (magazine.rs `loadout`): the rifle in hand, the pistol beside it, full
            // magazines and their spare rounds.
            assert_eq!((p.ammo, p.reserve, p.owned, p.weapon, p.gun, p.carry, p.hp), ([-1, 12, 0, 0, 0, 0, 60, 0], [0, 48, 0, 0, 0, 0, 180, 0], [false, true, false, false, false, false, true, false], RIFLE, RIFLE, -1, 100.));
        }
        assert_eq!(team(&s, 0).iter().map(|id| who(&s, *id).p[2]).collect::<Vec<_>>(), [2.9, 3.1, 3.3]);
        assert_eq!(team(&s, 1).iter().map(|id| who(&s, *id).p[0]).collect::<Vec<_>>(), [2.75, 2.75, 2.75]);
        // Every hotdog is named after its team; nobody on the ketchup side is called MUSTARD. Only the first is a person's.
        assert_eq!(s.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["DIJON", "TOMATO", "HONEY", "CATSUP", "GRAINY", "PASSATA"]);
        assert_eq!(Feast::with_map(map(), false).players.iter().map(|p| p.bot).collect::<Vec<_>>(), [false, true, true, true, true, true]);
        // A hotdog and the mode are written as the server writes them.
        let seen = serde_json::to_value(&s.players).unwrap();
        for (i, p) in seen.as_array().unwrap().iter().enumerate() {
            assert_eq!((p["team"].as_u64(), p["carry"].as_i64(), p["ready"].as_bool()), (Some(i as u64 % 2), Some(-1), Some(false)));
            assert!(p.get("job").is_none());
        }
        let state = s.fort_state().unwrap();
        assert_eq!((state.scores, state.rounds, state.match_no, state.field, state.build, state.ends), ([0, 0], ROUNDS, 1, true, 5., 5.));
        assert_eq!(state.bacon, BaconState { state: HOME, holder: 0, p: [0., DECK, 3.1], idle: 0. });
        let json = serde_json::to_value(&state).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["alarms", "bacon", "build", "champion", "ends", "field", "match", "reason", "rounds", "scores", "stale", "winner"]);
        assert!(json["winner"].is_null() && json["champion"].is_null());
        // The server's own words are read back.
        let theirs: FortState = serde_json::from_str(r#"{"scores":[1,2],"rounds":3,"match":2,"build":150,"ends":150,"field":false,"bacon":{"state":1,"holder":4,"p":[0.5,0.9025,3],"idle":0},"stale":12.5,"alarms":1,"winner":-1,"reason":"wipe","champion":null}"#).unwrap();
        assert_eq!((theirs.scores, theirs.match_no, theirs.field, theirs.bacon.holder, theirs.winner, theirs.champion, theirs.reason.as_str()), ([1, 2], 2, false, 4, Some(-1), None, "wipe"));
        // The map's own build time is used unless another is set.
        assert_eq!(Feast::with_map(map(), false).fort.unwrap().build, 150.);
    }

    #[test]
    fn the_siege_is_as_it_was_five_hotdogs_no_teams_no_forts() {
        let mut s = Feast::new(false);
        assert!(s.fort.is_none() && s.fort_state().is_none());
        assert_eq!((s.players.len(), s.stage), (5, Stage::Warmup));
        assert!(s.players.iter().all(|p| p.team == 0 && p.carry == -1 && !p.ready));
        assert!(!s.ready(1, true) && !s.allied(1, 2));
        s.lower_field();
        s.set_build(3.);
        assert_eq!(s.stage, Stage::Warmup);
        // A hotdog read from a snapshot that says nothing of carrying carries nothing.
        let old: Player = serde_json::from_str(r#"{"id":3,"hp":50}"#).unwrap();
        assert_eq!((old.carry, old.team, old.ready), (-1, 0, false));
    }

    #[test]
    fn while_the_forcefield_stands_it_stops_hotdogs_routes_rounds_and_thrown_objects_and_nobody_can_be_hurt() {
        let mut s = fort(30.);
        assert!(s.fort.as_ref().unwrap().up);
        // Walking: a body steered at the other side stops at the field (0.03 m short of its face, as at any
        // block), even in one long walk.
        at(&mut s, 1, -0.3, 2.5);
        walk(&mut s, 1, [1., 2.5], 90);
        let x = who(&s, 1).p[0];
        assert!(x < -0.069 && x > -0.12, "stopped at {x}");
        let before = who(&s, 1).p;
        s.walk(1, Vec2::new(0.2, 0.), 0.);
        assert!(who(&s, 1).p[0] <= before[0] + 1e-6, "stopped at the field: {:?}", who(&s, 1).p);
        s.walk(1, Vec2::new(-0.1, 0.), 0.);
        assert!(near(who(&s, 1).p[0], before[0] - 0.1), "a step on its own side is taken");
        // No route leads across, so a bot's errands and a cat's hunt stay on one side.
        assert!(s.level.path([-0.5, 2.5], [0.5, 2.5]).is_empty());
        assert!(!s.level.joined([-0.5, 2.5], [0.5, 2.5]));
        // Nobody can fire or strike while it stands, with either gun; a round that is in the air stops at it,
        // and the rival beyond is not hurt; nor is anybody by anything else.
        at(&mut s, 1, -0.5, 2.5);
        at(&mut s, 2, 0.3, 2.5);
        let d = look(&mut s, 1, [1., 0., 0.]);
        for weapon in [RIFLE, 1] {
            s.select(1, weapon);
            s.player_mut(1).unwrap().cooldown = 0.;
            assert!(!s.fire(1, d, weapon) && !s.melee(1, d), "weapon {weapon}");
        }
        let p = who(&s, 1);
        assert_eq!((s.projectiles.len(), s.shots.len(), p.swings, p.ammo[1], p.ammo[RIFLE]), (0, 0, 0, 12, 60));
        let o = muzzle(p);
        s.launch(1, o, d, 1);
        land(&mut s);
        assert!(near(s.splats.last().unwrap().p[0], -0.04), "the round ended at {}", s.splats.last().unwrap().p[0]);
        s.hurt(2, 60.);
        s.wound(1, 60.);
        assert_eq!((who(&s, 2).hp, who(&s, 1).hp), (100., 100.));
        // A pillow thrown at it (by the trigger: throwing is building, not a weapon) falls on the thrower's side.
        pose(&mut s, PILLOW, [-0.35, F, 2.5]);
        at(&mut s, 1, -0.5, 2.5);
        let d = look(&mut s, 1, [1., -0.3, 0.]);
        assert!(s.grab(1, d));
        let d = look(&mut s, 1, [1., 0.2, 0.]);
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(s.fire(1, d, RIFLE));
        assert!(who(&s, 1).carry == -1 && s.props.velocity(PILLOW)[0] > 1.);
        run(&mut s, 3.);
        assert!(pos(&s, PILLOW)[0] < -0.04, "the pillow came down at {}", pos(&s, PILLOW)[0]);
    }

    #[test]
    fn the_forcefield_drops_when_the_build_time_is_up_the_level_opens_the_guns_unlock_rounds_and_thrown_objects_cross_and_hotdogs_can_be_hurt() {
        let mut s = fort(5.);
        // (The pickups are on their pads from the start: see the next test.)
        assert!(s.pickups.iter().all(|item| item.ready == 0.));
        assert!(s.alert.starts_with("BUILD YOUR BASE. THE FORCEFIELD DROPS IN 0:05."), "{}", s.alert);
        run(&mut s, 4.9);
        assert_eq!(s.stage, Stage::Build);
        run(&mut s, 0.2);
        assert_eq!(s.stage, Stage::Fight);
        assert!(!s.fort.as_ref().unwrap().up && !s.fort_state().unwrap().field);
        assert!(s.alert.contains("FORCEFIELD IS DOWN"));
        assert!(s.pickups.iter().all(|item| item.ready == 0.));
        assert!(!s.level.path([-0.5, 2.5], [0.5, 2.5]).is_empty());
        assert!(s.level.joined([-0.5, 2.5], [0.5, 2.5]));
        at(&mut s, 1, -0.5, 2.5);
        at(&mut s, 2, 0.3, 2.5);
        // (Facing away: a level round meets the back of its head, the body's damage, not the face's.)
        s.player_mut(2).unwrap().yaw = -std::f32::consts::FRAC_PI_2;
        let d = look(&mut s, 1, [1., 0., 0.]);
        assert!(s.fire(1, d, 1));
        land(&mut s);
        assert_eq!(who(&s, 2).hp, 100. - PLAYER_DAMAGE[1]);
        pose(&mut s, PILLOW, [-0.35, F, 2.5]);
        let d = look(&mut s, 1, [1., -0.3, 0.]);
        assert!(s.grab(1, d));
        let d = look(&mut s, 1, [1., 0.2, 0.]);
        assert!(s.toss(1, d));
        run(&mut s, 3.);
        assert!(pos(&s, PILLOW)[0] > 0.04, "the pillow came down at {}", pos(&s, PILLOW)[0]);
        // A hotdog walks through where it stood.
        at(&mut s, 1, -0.3, 2.5);
        walk(&mut s, 1, [1., 2.5], 90);
        assert!(who(&s, 1).p[0] > 0.2, "walked to {}", who(&s, 1).p[0]);
    }

    /// Puts hotdog `id` on the pad of pickup `item` and has it take it.
    fn take(s: &mut Feast, id: u32, item: usize) -> bool {
        let pad = s.pickups[item].p;
        at(s, id, pad[0], pad[2]);
        s.pickup(id, item)
    }
    // The fixture's pickups: each side has a sniper, a hot sauce thrower, a bazooka and two lots of mustard (the
    // mustard side's first, at -x), then the mustard side's syrup rifle and box of ammunition, then the ketchup side's.
    const SNIPER: usize = 0;
    const SAUCE: usize = 1;
    const BAZOOKA: usize = 2;
    const SAUCE_KETCHUP: usize = 6;
    const BOX: usize = 11;

    #[test]
    fn the_pickups_are_on_their_pads_from_the_start_of_the_build_each_side_can_take_only_its_own_and_the_guns_stay_locked_until_the_field_drops() {
        let mut s = fort(5.);
        assert_eq!(s.stage, Stage::Build);
        assert!(s.pickups.iter().all(|item| item.ready <= s.elapsed), "every pickup is on its pad");
        assert_eq!([SNIPER, SAUCE, BAZOOKA, SAUCE_KETCHUP, BOX].map(|i| s.pickups[i].weapon), [2, 3, 4, 3, crate::map::AMMO_BOX]);
        // Hotdog 1, a Mustard Eater, takes the sniper on its own side; it comes to hand.
        assert!(take(&mut s, 1, SNIPER));
        assert_eq!((who(&s, 1).owned[2], who(&s, 1).ammo[2], who(&s, 1).weapon), (true, 4, 2));
        // It cannot fire it, nor strike, while the field stands.
        let d = look(&mut s, 1, [1., 0., 0.]);
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(!s.fire(1, d, 2) && !s.melee(1, d));
        assert_eq!((s.projectiles.len(), who(&s, 1).ammo[2]), (0, 4));
        // A Ketchup Connoisseur put on a pad on the mustard side cannot take its pickup while the field divides them;
        // one on its own side it can.
        assert!(!take(&mut s, 2, SAUCE), "not across the field");
        assert!(!who(&s, 2).owned[3]);
        assert!(take(&mut s, 2, SAUCE_KETCHUP));
        assert!(who(&s, 2).owned[3]);
        // The field drops: the guns fire, the pickups are as they were (the sniper taken is still on its way back),
        // and either side can take what is on either side.
        let back = s.pickups[SNIPER].ready;
        at(&mut s, 1, -0.5, 2.5);
        run(&mut s, 5.1);
        assert_eq!(s.stage, Stage::Fight);
        assert!(s.pickups[SNIPER].ready == back && back > s.elapsed + 20.);
        let d = look(&mut s, 1, [-1., 0., 0.]);
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(s.fire(1, d, 2));
        assert_eq!(who(&s, 1).ammo[2], 3);
        assert!(take(&mut s, 2, SAUCE), "once the field is down");
    }

    #[test]
    fn power_weapons_are_scarce_in_forts_as_in_every_gun_mode_a_sniper_pickup_gives_4_rounds_a_bazooka_2_a_box_of_ammunition_tops_up_only_the_other_guns_and_one_run_dry_stays_in_hand_empty() {
        let mut s = fight();
        let given: Vec<(usize, i32)> = s.pickups.iter().take(5).map(|item| (item.weapon, item.ammo)).collect();
        assert_eq!(given, [(2, 4), (3, 90), (4, 2), (1, 12), (1, 12)], "the map says the sniper 5, the hot sauce 80 and the bazooka 3: the rules' table wins");
        // The sniper in place of the rifle in hand, then the bazooka in place of the pistol.
        assert!(take(&mut s, 1, SNIPER));
        s.select(1, 1);
        assert!(take(&mut s, 1, BAZOOKA));
        let p = who(&s, 1);
        assert_eq!((p.owned, p.ammo[2], p.ammo[4]), ([false, false, true, false, true, false, false, false], 4, 2));
        // Four shots and the sniper has run dry: it stays in hand, empty, and the trigger does nothing.
        at(&mut s, 1, -0.5, 2.5);
        let d = look(&mut s, 1, [-1., 0., 0.]);
        for shot in 0..5 {
            s.player_mut(1).unwrap().cooldown = 0.;
            assert_eq!(s.fire(1, d, 2), shot < 4, "shot {shot}");
        }
        let p = who(&s, 1);
        assert_eq!((p.owned[2], p.ammo[2], p.weapon), (true, 0, 2));
        // A box of ammunition gives the power weapons nothing: with only those in hand it is left where it is.
        assert!(!take(&mut s, 1, BOX));
        assert_eq!((who(&s, 1).ammo[2], who(&s, 1).ammo[4], s.pickups[BOX].ready), (0, 2, 0.));
        // Beside another gun it tops that one's reserve up (magazine.rs), and still not the power weapon.
        let p = s.player_mut(1).unwrap();
        (p.owned, p.ammo, p.reserve) = ([false, true, false, false, true, false, false, false], [-1, 3, 0, 0, 1, 0, 0, 0], [0; feast::GUNS]);
        assert!(take(&mut s, 1, BOX));
        assert_eq!((who(&s, 1).ammo[1], who(&s, 1).reserve[1], who(&s, 1).ammo[4], who(&s, 1).reserve[4]), (3, CARRY[1], 1, 0));
        // A bot does not go for a box for its power weapon alone; with its rifle low it does.
        let mut low = who(&s, 3).clone();
        (low.owned, low.ammo, low.reserve) = ([false, false, true, false, false, false, true, false], [-1, 0, 0, 0, 0, 0, 60, 0], [0; feast::GUNS]);
        assert!(!Feast::wants(&low, &s.pickups[BOX]));
        low.ammo[RIFLE] = 10;
        assert!(Feast::wants(&low, &s.pickups[BOX]));
        // Free For All on a Forts map has no forts, and the pickups are the same: the sniper 4, the bazooka 2, and a box
        // does not top a sniper up.
        let mut s = Feast::with_map(map(), false);
        s.set_rules(crate::rules::Rules::parse("mode=ffa"));
        assert!(s.fort.is_none());
        assert_eq!((s.pickups[SNIPER].ammo, s.pickups[BAZOOKA].ammo, s.pickups[SNIPER].respawn), (4, 2, 20.));
        let p = s.player_mut(1).unwrap();
        (p.owned, p.ammo, p.reserve) = ([false, false, true, false, false, false, true, false], [-1, 0, 1, 0, 0, 0, 10, 0], [0, 0, 0, 0, 0, 0, 20, 0]);
        assert!(take(&mut s, 1, BOX));
        assert_eq!((who(&s, 1).reserve[2], who(&s, 1).reserve[RIFLE]), (0, CARRY[RIFLE]));
    }

    #[test]
    fn a_power_weapon_comes_back_30_to_60_seconds_after_it_is_taken_drawn_from_the_games_seed_each_time_and_the_other_pickups_as_before() {
        // Each pickup taken at the same moment of a fight with the given seed: how long each takes to come back.
        let draw = |seed: u64| {
            let mut s = fight();
            s.card_seed = seed;
            s.player_mut(1).unwrap().ammo[RIFLE] = 5;
            let taken = s.elapsed;
            let backs: Vec<f32> = [SNIPER, BAZOOKA, SAUCE, BOX]
                .into_iter()
                .map(|item| {
                    s.select(1, 1);
                    assert!(take(&mut s, 1, item), "pickup {item}");
                    // The pad's ring and the head-up display count back from the time drawn: it is the pickup's
                    // `respawn` until it is taken again.
                    assert_eq!(s.pickups[item].ready, taken + s.pickups[item].respawn);
                    s.pickups[item].respawn
                })
                .collect();
            (s, backs)
        };
        let mut drawn = vec![];
        for seed in 0..6 {
            let (_, backs) = draw(seed);
            assert_eq!(backs, draw(seed).1, "the same seed draws the same times");
            assert!(backs[..2].iter().all(|t| (POWER_BACK[0]..=POWER_BACK[1]).contains(t)), "{backs:?}");
            assert_eq!(backs[2..], [20., 12.], "the hot sauce and the box as before");
            drawn.extend_from_slice(&backs[..2]);
        }
        drawn.sort_by(f32::total_cmp);
        drawn.dedup();
        assert!(drawn.len() >= 10, "the times vary: {drawn:?}");
        // Not back before its time; taken again, its time is drawn afresh.
        let (mut s, backs) = draw(7);
        let again = s.pickups[SNIPER].ready;
        s.elapsed = again - 0.5;
        s.select(1, 1);
        assert!(!take(&mut s, 1, SNIPER));
        s.elapsed = again;
        assert!(take(&mut s, 1, SNIPER));
        let next = s.pickups[SNIPER].respawn;
        assert!((POWER_BACK[0]..=POWER_BACK[1]).contains(&next) && next != backs[0], "{next} after {}", backs[0]);
        assert_eq!(s.pickups[SNIPER].ready, again + next);
    }

    #[test]
    fn whoever_takes_a_pickup_first_has_it_and_a_second_hotdog_on_the_pad_in_the_same_step_gets_nothing() {
        for building in [true, false] {
            let mut s = if building { fort(30.) } else { fight() };
            // Two Mustard Eaters on the sniper's pad, one after the other in the step's order.
            assert!(take(&mut s, 3, SNIPER));
            assert!(!take(&mut s, 1, SNIPER));
            assert!(who(&s, 3).owned[2] && !who(&s, 1).owned[2]);
            assert_eq!((who(&s, 3).ammo[2], who(&s, 1).ammo[2]), (4, 0));
            // The same for a box of ammunition: the first tops up, the second gets nothing.
            for id in [1, 5] {
                s.player_mut(id).unwrap().reserve[RIFLE] = 1;
            }
            assert!(take(&mut s, 5, BOX));
            assert!(!take(&mut s, 1, BOX));
            assert_eq!((who(&s, 5).reserve[RIFLE], who(&s, 1).reserve[RIFLE]), (CARRY[RIFLE], 1));
        }
    }

    #[test]
    fn a_loose_object_in_one_hotdogs_hands_cannot_be_taken_out_of_them_by_another() {
        for building in [true, false] {
            let mut s = if building { fort(30.) } else { fight() };
            pose(&mut s, PILLOW, [-0.35, F, 2.5]);
            at(&mut s, 1, -0.5, 2.5);
            let d = look(&mut s, 1, [1., -0.3, 0.]);
            assert!(s.grab(1, d));
            let held = pos(&s, PILLOW);
            // A team mate, and (in the fight) a rival, face it from just beyond it and reach for it, by looking at it
            // and by standing beside it: nothing.
            let others: &[u32] = if building { &[3] } else { &[3, 2] };
            for &id in others {
                at(&mut s, id, held[0] + 0.12, 2.5);
                let d = look(&mut s, id, [-1., -0.5, 0.]);
                assert!(!s.grab(id, d), "hotdog {id} looking at it");
                assert!(!s.grab(id, Vec3::ZERO), "hotdog {id} beside it");
                assert_eq!(who(&s, id).carry, -1);
            }
            run(&mut s, 0.2);
            assert_eq!((who(&s, 1).carry, s.props.state(PILLOW), s.props.body(PILLOW).holder), (PILLOW as i32, State::Carried, 1));
        }
    }

    #[test]
    fn the_build_phase_ends_early_only_when_every_living_person_is_ready_on_both_teams_and_bots_are_not_asked() {
        // Hotdogs 1 and 2 are people, one on each team; the rest are bots, building.
        let mut s = Feast::with_map(map(), false);
        s.set_build(60.);
        s.player_mut(2).unwrap().bot = false;
        run(&mut s, 1.);
        assert!(s.alert.contains("0 OF 2 READY"), "{}", s.alert);
        assert!(s.ready(1, true));
        run(&mut s, 1.);
        assert_eq!(s.stage, Stage::Build);
        assert!(s.alert.contains("1 OF 2 READY"), "{}", s.alert);
        assert!(!s.ready(3, true), "a bot is not asked");
        assert!(s.ready(1, false) && s.ready(2, true));
        run(&mut s, 1.);
        assert_eq!(s.stage, Stage::Build);
        assert!(s.ready(1, true));
        s.tick(DT);
        assert_eq!(s.stage, Stage::Fight);
        assert!(s.elapsed < 4.);
        assert_eq!((who(&s, 1).ready, who(&s, 2).ready), (false, false));
        // In the fight nobody is asked any more.
        assert!(!s.ready(1, true));
    }

    #[test]
    fn a_hotdog_picks_up_the_loose_object_it_looks_at_or_the_nearest_beside_it_holds_it_in_front_walks_slower_under_a_heavy_one_and_its_guns_are_away() {
        let mut s = fort(60.);
        // Looking at a crate within reach; one out of reach is left.
        pose(&mut s, CRATE, [-1., F, 2.4]);
        at(&mut s, 1, -1.5, 2.4);
        let d = look(&mut s, 1, [1., -0.4, 0.]);
        run(&mut s, 0.5);
        assert!(!s.grab(1, d), "too far");
        at(&mut s, 1, -1.2, 2.4);
        assert!(s.grab(1, d));
        assert_eq!((who(&s, 1).carry, s.props.state(CRATE), who(&s, 1).weapon), (CRATE as i32, State::Carried, 0));
        let (held, p) = (pos(&s, CRATE), who(&s, 1).p);
        assert!(near(held[0], p[0] + OUT[1] as f32) && near(held[1], F + LIFT as f32) && near(held[2], 2.4), "held at {held:?}");
        // It goes where the hotdog goes and turns with it.
        at(&mut s, 1, -1.6, 2.6);
        let d = look(&mut s, 1, [0., 0., 1.]);
        s.tick(DT);
        assert!(near(pos(&s, CRATE)[2], 2.6 + OUT[1] as f32) && near(pos(&s, CRATE)[0], -1.6));
        // Hands full: no shot (the trigger throws), no strike, no change of weapon, and no second object.
        assert!(!s.melee(1, d));
        s.select(1, 1);
        assert_eq!(who(&s, 1).weapon, 0);
        assert!(!s.grab(1, d) && s.laden(1));
        // A crate is heavy: its carrier walks at SLOW of the pace.
        assert!(s.props.shape(s.props.body(CRATE).shape).mass >= HEAVY);
        assert_eq!(s.burden(1), SLOW);
        let x = who(&s, 1).p[0];
        walk(&mut s, 1, [0., 2.6], 30);
        assert!((who(&s, 1).p[0] - x - actor::BOT_WALK as f32 * SLOW).abs() < 0.02, "walked {}", who(&s, 1).p[0] - x);
        // A pillow is not.
        assert!(s.put(1, d));
        assert_eq!((who(&s, 1).weapon, who(&s, 1).gun), (RIFLE, RIFLE));
        let [x, _, z] = who(&s, 1).p;
        pose(&mut s, PILLOW, [x, F, z + 0.12]);
        let d = look(&mut s, 1, [0., -0.8, 1.]);
        assert!(s.grab(1, d));
        assert_eq!((who(&s, 1).carry, s.burden(1)), (PILLOW as i32, 1.));
        // Beside it, without looking at it: the nearest object within reach of the feet.
        let d = look(&mut s, 1, [1., -3., 0.]);
        assert!(s.put(1, d));
        let d = look(&mut s, 1, [-1., 0.9, 0.]);
        assert!(s.grab(1, d), "the one beside it");
        assert_eq!(s.props.state(who(&s, 1).carry as usize), State::Carried);
        // The roster says who carries what.
        s.tick(DT);
        assert_eq!(serde_json::to_value(&s.players).unwrap()[0]["carry"].as_i64(), Some(who(&s, 1).carry as i64));
    }

    #[test]
    fn putting_down_lets_the_object_down_gently_where_it_is_held_the_same_spot_every_time_on_top_of_what_is_there_with_the_hotdogs_pace_and_throwing_sends_it_along_the_aim() {
        let mut s = fort(60.);
        pose(&mut s, CRATE, [-1., F, 2.4]);
        pose(&mut s, CRATE + 1, [-1., F, 2.8]);
        at(&mut s, 1, -1.2, 2.4);
        look(&mut s, 1, [1., -0.4, 0.]);
        run(&mut s, 0.5);
        // Standing still: let down OUT ahead, straight below where it was held, whatever the aim, and it comes to
        // rest there without a bounce or a tumble.
        let mut spots = vec![];
        for aim in [[1., -0.4, 0.], [1., 0., 0.], [1., 0.5, 0.], [1., -3., 0.]] {
            at(&mut s, 1, -1.2, 2.4);
            let d = look(&mut s, 1, [1., -0.4, 0.]);
            s.tick(DT);
            assert!(s.grab(1, d));
            at(&mut s, 1, -1.5, 2.6);
            let d = look(&mut s, 1, aim);
            run(&mut s, 0.3);
            let feet = who(&s, 1).p;
            let spot = s.placement(1).unwrap();
            assert!(s.put(1, d));
            assert_eq!((who(&s, 1).carry, who(&s, 1).weapon), (-1, RIFLE));
            // (Snapped flush to whatever is within 2 cm of where it is held, square to the facing.)
            let at0 = pos(&s, CRATE);
            assert!((at0[0] - feet[0] - OUT[1] as f32).abs() < 0.021 && (at0[2] - feet[2]).abs() < 0.021 && at0[1] - F < 0.001, "let down at {at0:?} from {feet:?}");
            assert!((0..3).all(|k| (at0[k] as f64 - spot.p[k]).abs() < 1e-6) && spot.lock, "where the placement said: {spot:?}");
            assert_eq!((s.props.velocity(CRATE), s.props.state(CRATE)), ([0.; 3], State::Locked));
            let mut high = 0f32;
            for _ in 0..60 {
                s.tick(DT);
                high = high.max(pos(&s, CRATE)[1] - F);
            }
            assert_eq!(s.props.state(CRATE), State::Locked);
            assert!(high < 0.001 && s.props.up(CRATE)[1] > 0.999, "it rose {high} or tumbled");
            spots.push(pos(&s, CRATE).map(|v| (v * 1e4).round()));
            pose(&mut s, CRATE, [-1., F, 2.4]);
        }
        assert!(spots.iter().all(|q| *q == spots[0]), "the same spot each time: {spots:?}");
        // Put down on another: it stands on top (a wall is stacked this way), even when held a little off its
        // middle (it is drawn into line with the one under it), and it is locked there.
        let z = 2.6;
        pose(&mut s, CRATE, [-1.39, F, z]);
        run(&mut s, 1.);
        at(&mut s, 1, -0.85, 2.8);
        let d = look(&mut s, 1, [-1., -0.4, 0.]);
        run(&mut s, 0.2);
        assert!(s.grab(1, d));
        assert_eq!(who(&s, 1).carry, CRATE as i32 + 1);
        at(&mut s, 1, -1.36 - OUT[1] as f32, z + 0.02);
        let d = look(&mut s, 1, [1., 0., 0.]);
        run(&mut s, 0.3);
        assert!(s.put(1, d));
        run(&mut s, 3.);
        let top = pos(&s, CRATE + 1);
        let under = pos(&s, CRATE);
        assert!((top[1] - (F + 0.1)).abs() < 0.002 && (top[0] - under[0]).abs() < 1e-4 && (top[2] - under[2]).abs() < 1e-4, "stacked at {top:?} on {under:?}");
        assert_eq!((s.props.state(CRATE), s.props.state(CRATE + 1)), (State::Asleep, State::Locked));
        // On the move it keeps the hotdog's pace and slides on a little: further than standing still, and never
        // flung (no faster than KEEP, no lift, no spin).
        at(&mut s, 1, -1., 3.4);
        pose(&mut s, PILLOW, [-0.85, F, 3.4]);
        let d = look(&mut s, 1, [1., -0.6, 0.]);
        run(&mut s, 0.3);
        assert!(s.grab(1, d));
        walk(&mut s, 1, [0., 3.4], 20);
        let (pace, from) = (who(&s, 1).pace[0], who(&s, 1).p[0]);
        assert!(pace > 0.25 && pace <= KEEP, "pace {pace}");
        assert!(s.put(1, d));
        let v = s.props.velocity(PILLOW);
        assert!((v[0] - pace as f64).abs() < 1e-6 && v[1] == 0. && s.props.angular_velocity(PILLOW) == [0.; 3]);
        run(&mut s, 3.);
        let x = pos(&s, PILLOW)[0];
        assert!(x > from + OUT[2] as f32 + 0.01 && x < from + OUT[2] as f32 + 0.4, "it slid on to {x} from {}", from + OUT[2] as f32);
        assert_eq!(s.props.state(PILLOW), State::Locked, "and locked where it stopped");
        // Thrown, by the trigger: it leaves along the aim (a heavy object slower than a light one), and the
        // throw is a swing.
        at(&mut s, 1, -0.9, 3.45);
        pose(&mut s, PILLOW, [-0.8, F, 3.45]);
        let d = look(&mut s, 1, [1., -0.5, 0.]);
        run(&mut s, 0.2);
        assert!(s.grab(1, d));
        assert_eq!(who(&s, 1).carry, PILLOW as i32);
        let d = look(&mut s, 1, [1., 0., 0.]);
        let swings = who(&s, 1).swings;
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(s.fire(1, d, RIFLE));
        assert_eq!((who(&s, 1).carry, who(&s, 1).swings, who(&s, 1).weapon), (-1, swings + 1, RIFLE));
        assert!((s.props.velocity(PILLOW)[0] - THROW[0]).abs() < 1e-9 && s.props.velocity(PILLOW)[1] > 0.);
        assert!(s.projectiles.is_empty(), "no round was fired");
        run(&mut s, 3.);
        assert!(pos(&s, PILLOW)[0] > -0.5, "it landed at {}", pos(&s, PILLOW)[0]);
        // Picked up and put down in turn, and thrown.
        at(&mut s, 1, -1.5, z);
        let d = look(&mut s, 1, [1., -0.6, 0.]);
        run(&mut s, 0.2);
        assert!(s.grab(1, d));
        let got = who(&s, 1).carry as usize;
        assert!(got == CRATE || got == CRATE + 1, "got {got}");
        assert!(s.put(1, d));
        assert_eq!((who(&s, 1).carry, s.props.state(got)), (-1, State::Locked));
        run(&mut s, 2.);
        assert!(s.grab(1, d));
        let thrown = who(&s, 1).carry as usize;
        let d = look(&mut s, 1, [0.2, -0.3, 0.]);
        assert!(s.toss(1, d));
        assert_eq!(who(&s, 1).carry, -1);
        assert!((s.props.velocity(thrown)[0] - THROW[1] * (0.2f32 / 0.2f32.hypot(0.3)) as f64).abs() < 1e-6);
    }

    #[test]
    fn a_carried_object_cannot_be_put_or_carried_through_a_wall_and_its_carrier_dying_lets_it_fall() {
        let mut s = fort(60.);
        pose(&mut s, PILLOW, [-0.3, F, 2.5]);
        at(&mut s, 1, -0.4, 2.5);
        let d = look(&mut s, 1, [1., -0.6, 0.]);
        assert!(s.grab(1, d));
        // Against the forcefield: held short of it, and put down short of it.
        at(&mut s, 1, -0.12, 2.5);
        let d = look(&mut s, 1, [1., 0., 0.]);
        s.tick(DT);
        assert!(pos(&s, PILLOW)[0] < -0.04);
        assert!(s.put(1, d));
        assert!(pos(&s, PILLOW)[0] < -0.04);
        run(&mut s, 5.);
        s.drop_field();
        eprintln!("DBGW hotdog {:?} pillow {:?} {:?} d {d:?}", who(&s, 1).p, pos(&s, PILLOW), s.props.state(PILLOW));
        assert!(s.grab(1, d));
        let prop = who(&s, 1).carry as usize;
        s.hurt(1, 1000.);
        assert_eq!((who(&s, 1).alive, who(&s, 1).carry, s.props.state(prop)), (false, -1, State::Awake));
    }

    #[test]
    fn a_wall_built_of_crates_stops_a_round_and_a_hotdog_cannot_walk_through_it_it_has_to_go_round() {
        let mut s = fight();
        // Three crates carried one by one and put down in a row across the rival's way.
        let row = [(0, -1.2), (1, -1.09), (2, -0.98)];
        for (i, x) in row {
            pose(&mut s, CRATE + i, [-2., F, 2.4]);
            at(&mut s, 1, -2.15, 2.4);
            let d = look(&mut s, 1, [1., -0.5, 0.]);
            s.tick(DT);
            assert!(s.grab(1, d));
            at(&mut s, 1, x, 2.4 + OUT[1] as f32);
            let d = look(&mut s, 1, [0., 0., -1.]);
            s.tick(DT);
            assert!(s.put(1, d));
            run(&mut s, 1.);
        }
        run(&mut s, 2.);
        for (i, _) in row {
            assert_eq!(s.props.state(CRATE + i), State::Locked);
        }
        // The rival's round stops at the wall; the hotdog behind it is not hurt.
        at(&mut s, 1, -1.5, 2.4);
        at(&mut s, 2, -0.5, 2.4);
        let d = look(&mut s, 2, [-1., -0.2, 0.]);
        assert!(s.fire(2, d, 1));
        land(&mut s);
        assert!(s.splats.last().unwrap().p[0] > -0.95, "stopped at {}", s.splats.last().unwrap().p[0]);
        assert_eq!(who(&s, 1).hp, 100.);
        // Walking straight at it, the rival is turned aside: it is never where the wall stands, the wall
        // hardly gives, and its way past is round the end.
        at(&mut s, 1, -2.5, 3.5);
        let mut aside = 0f32;
        for _ in 0..150 {
            walk(&mut s, 2, [-2., 2.4], 1);
            let [x, _, z] = who(&s, 2).p;
            assert!(!(x > -1.25 && x < -0.93 && (z - 2.4).abs() < 0.05), "inside the wall at {x}, {z}");
            if x > -1.3 && x < -0.9 {
                aside = aside.max((z - 2.4).abs());
            }
        }
        assert!(aside > 0.09, "it passed {aside} m to the side");
        for (i, x) in row {
            assert!((pos(&s, CRATE + i)[0] - x).abs() < 0.04 && (pos(&s, CRATE + i)[2] - 2.4).abs() < 0.04, "the wall held");
        }
        // A single pillow is no wall: it is shoved along.
        pose(&mut s, PILLOW, [0.5, F, 2.6]);
        at(&mut s, 2, 0.8, 2.6);
        run(&mut s, 0.5);
        walk(&mut s, 2, [-0.5, 2.6], 90);
        assert!(who(&s, 2).p[0] < 0.2 && pos(&s, PILLOW)[0] < 0.3, "the pillow was pushed");
    }

    #[test]
    fn team_mates_cannot_hurt_each_other_with_rounds_bursts_or_blows_and_rivals_can() {
        let mut s = fight();
        for id in 1..=6 {
            let x = if id % 2 == 0 { 2. + id as f32 * 0.1 } else { -2. - id as f32 * 0.1 };
            at(&mut s, id, x, 3.8);
        }
        assert!(s.allied(1, 3) && !s.allied(1, 2));
        // A round flies on past a team mate and lands on the rival behind.
        at(&mut s, 1, -1., 2.5);
        at(&mut s, 3, -0.8, 2.5);
        at(&mut s, 2, -0.5, 2.5);
        // (Facing away: a level round meets the back of its head, the body's damage, not the face's.)
        s.player_mut(2).unwrap().yaw = -std::f32::consts::FRAC_PI_2;
        let d = look(&mut s, 1, [1., 0., 0.]);
        assert!(s.fire(1, d, 1));
        land(&mut s);
        assert_eq!((who(&s, 3).hp, who(&s, 2).hp, who(&s, 2).last_damage.attacker), (100., 100. - PLAYER_DAMAGE[1], 1));
        // A glob bursts between them: only the rival is hurt, and only the rival is thrown.
        {
            let p = s.player_mut(1).unwrap();
            (p.owned[4], p.ammo[4], p.cooldown) = (true, 1, 0.);
        }
        s.player_mut(2).unwrap().hp = 100.;
        at(&mut s, 3, -0.42, 2.62);
        run(&mut s, 0.2);
        assert!(s.fire(1, d, 4));
        land(&mut s);
        assert_eq!(who(&s, 3).hp, 100.);
        assert!(who(&s, 2).hp <= 100. - PLAYER_DAMAGE[4] + 1e-4);
        assert!(!s.body(3).unwrap().launched);
        // (A glob kills: the rival is back for the strike.)
        (s.player_mut(2).unwrap().hp, s.player_mut(2).unwrap().alive) = (100., true);
        s.embody();
        // A strike: a team mate in reach is not touched; a rival is hit for PLAYER_DAMAGE[0] (half of the most a hotdog
        // has) from in front of it, and killed from behind (melee_tests.rs has the arc, the stances and the rest).
        s.player_mut(1).unwrap().cooldown = 0.;
        s.player_mut(2).unwrap().hp = 100.;
        at(&mut s, 2, 2., 2.5);
        at(&mut s, 3, -0.88, 2.5);
        s.tick(DT);
        assert!(s.melee(1, d));
        assert_eq!(who(&s, 3).hp, 100.);
        s.player_mut(1).unwrap().cooldown = 0.;
        at(&mut s, 3, -2., 2.5);
        at(&mut s, 2, -0.88, 2.5);
        s.tick(DT);
        // (Hotdog 2 faces the striker, which is at its -x.)
        s.player_mut(2).unwrap().yaw = std::f32::consts::FRAC_PI_2;
        assert!(s.melee(1, d));
        assert_eq!((who(&s, 2).hp, who(&s, 2).last_damage.kind, who(&s, 2).last_damage.attacker), (100. - PLAYER_DAMAGE[0], MELEE, 1));
        // With its back to the striker the same blow is a kill.
        s.player_mut(1).unwrap().cooldown = 0.;
        s.player_mut(2).unwrap().hp = 100.;
        s.player_mut(2).unwrap().yaw = -std::f32::consts::FRAC_PI_2;
        assert!(s.melee(1, d));
        assert_eq!((who(&s, 2).alive, who(&s, 2).last_damage.kind, who(&s, 2).last_damage.attacker, who(&s, 2).last_damage.amount), (false, feast::BACKSTAB, 1, 100.));
    }

    #[test]
    fn whoever_touches_the_bacon_carries_it_using_no_weapon_and_its_carrier_wins_the_round_in_their_own_base_and_nowhere_else() {
        let mut s = fight();
        assert_eq!((bacon(&s).state, bacon(&s).holder), (HOME, 0));
        // Not during the build, even standing on it.
        {
            let mut early = fort(60.);
            up(&mut early, 1, 0., 3.1);
            run(&mut early, 0.5);
            assert_eq!(bacon(&early).holder, 0);
        }
        up(&mut s, 1, -0.2, 3.1);
        s.tick(DT);
        assert_eq!(bacon(&s).holder, 0, "out of reach");
        up(&mut s, 1, -0.1, 3.1);
        s.tick(DT);
        assert_eq!((bacon(&s).state, bacon(&s).holder), (HELD, 1));
        assert!(s.alert.contains("DIJON OF THE MUSTARD EATERS HAS THE BACON"), "{}", s.alert);
        // Its guns are away and it uses no weapon: the trigger and the strike do nothing, and no gun can be drawn.
        // It can still take its stim.
        assert_eq!(who(&s, 1).weapon, 0);
        let d = look(&mut s, 1, [1., 0., 0.]);
        let swings = who(&s, 1).swings;
        assert!(!s.fire(1, d, 1));
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(!s.melee(1, d));
        assert_eq!((s.projectiles.len(), who(&s, 1).swings, who(&s, 1).ammo[1]), (0, swings, 12));
        assert!(s.stim(1));
        s.select(1, 1);
        assert_eq!(who(&s, 1).weapon, 0);
        assert!(!s.grab(1, d) && s.laden(1));
        // It slows its carrier: BACON_PACE of the pace (a person's, and a bot's).
        assert_eq!(s.burden(1), BACON_PACE);
        assert!(BACON_PACE < 1. && BACON_PACE > 0.5);
        assert!((s.tune.pace(false, false, false, s.burden(1)) - actor::WALK as f32 * BACON_PACE).abs() < 1e-6);
        assert_eq!(s.burden(3), 1., "nobody else");
        // It goes with its carrier, and the mode's state says where.
        at(&mut s, 1, 1., 2.6);
        s.tick(DT);
        assert_eq!(bacon(&s).p, who(&s, 1).p);
        assert_eq!(s.fort_state().unwrap().bacon, BaconState { state: HELD, holder: 1, p: who(&s, 1).p, idle: 0. });
        // In the rival's base nothing happens; in its own the round is won.
        at(&mut s, 1, 2.75, 3.1);
        run(&mut s, 0.5);
        assert_eq!(s.stage, Stage::Fight);
        at(&mut s, 1, -2.75, 3.1);
        s.tick(DT);
        assert_eq!(s.stage, Stage::RoundOver);
        let fort = s.fort.as_ref().unwrap();
        assert_eq!((fort.winner, fort.reason.as_str(), fort.scores, s.winner), (Some(0), "capture", [1, 0], Some(1)));
        assert!(s.alert.contains("DIJON BROUGHT HOME THE BACON. MUSTARD 1, KETCHUP 0."), "{}", s.alert);
    }

    #[test]
    fn a_carrier_who_dies_drops_the_bacon_where_they_fell_anyone_can_take_it_and_left_alone_it_goes_back_to_the_middle() {
        let mut s = fight();
        up(&mut s, 1, 0., 3.1);
        s.tick(DT);
        assert_eq!(bacon(&s).holder, 1);
        at(&mut s, 1, -1., 2.6);
        s.tick(DT);
        s.wound(1, 1000.);
        s.tick(DT);
        assert_eq!((bacon(&s).state, bacon(&s).holder, bacon(&s).p), (DROPPED, 0, [-1., F, 2.6]));
        run(&mut s, RETURN - 1.);
        assert_eq!(bacon(&s).state, DROPPED);
        // A rival takes it up, and loses it in turn; then nobody comes and it returns.
        at(&mut s, 2, -1.05, 2.6);
        s.tick(DT);
        assert_eq!((bacon(&s).state, bacon(&s).holder), (HELD, 2));
        at(&mut s, 2, 0.5, 2.5);
        s.tick(DT);
        s.wound(2, 1000.);
        s.tick(DT);
        assert_eq!((bacon(&s).state, bacon(&s).p), (DROPPED, [0.5, F, 2.5]));
        run(&mut s, RETURN - 0.2);
        assert_eq!(bacon(&s).state, DROPPED);
        run(&mut s, 0.4);
        assert_eq!((bacon(&s).state, bacon(&s).holder, bacon(&s).p), (HOME, 0, [0., DECK, 3.1]));
        assert!(s.alert.contains("BACK IN THE MIDDLE"));
        // A hotdog with its hands full of something else puts that down for the bacon.
        pose(&mut s, PILLOW, [-0.2, DECK, 3.1]);
        up(&mut s, 3, -0.25, 3.1);
        let d = look(&mut s, 3, [1., -0.8, 0.]);
        assert!(s.grab(3, d));
        up(&mut s, 3, 0., 3.1);
        s.tick(DT);
        assert_eq!((bacon(&s).holder, who(&s, 3).carry, s.props.state(PILLOW)), (3, -1, State::Locked));
    }

    #[test]
    fn a_team_with_nobody_left_has_lost_the_round_the_dead_stay_dead_until_the_next_round_and_both_teams_dying_at_once_is_nobodys_point() {
        let mut s = fight();
        s.wound(4, 1000.);
        s.wound(6, 1000.);
        run(&mut s, 2.);
        assert_eq!(s.stage, Stage::Fight);
        assert_eq!(team(&s, 1).iter().map(|id| who(&s, *id).alive).collect::<Vec<_>>(), [true, false, false]);
        s.wound(2, 1000.);
        s.tick(DT);
        assert_eq!(s.stage, Stage::RoundOver);
        let fort = s.fort.as_ref().unwrap();
        assert_eq!((fort.winner, fort.reason.as_str(), fort.scores, s.winner), (Some(0), "wipe", [1, 0], None));
        assert!(s.alert.contains("THE MUSTARD EATERS ARE THE LAST STANDING"), "{}", s.alert);
        // The dead do nothing and do not come back during the pause; the next round everyone is up again, in a
        // new build phase with everything put back.
        let was = who(&s, 2).p;
        assert!(s.walk(2, Vec2::new(0.01, 0.), 0.).is_none());
        assert!(!s.fire(2, Vec3::X, 1));
        run(&mut s, INTERMISSION - 1.);
        assert_eq!((s.stage, who(&s, 2).alive, who(&s, 2).p), (Stage::RoundOver, false, was));
        pose(&mut s, CRATE, [-1., F, 2.4]);
        run(&mut s, 1.2);
        let fort = s.fort.as_ref().unwrap();
        assert_eq!((s.stage, s.round, fort.scores, fort.up, fort.build, fort.ends), (Stage::Build, 2, [1, 0], true, 5., 5.));
        assert!(s.players.iter().all(|p| p.alive && p.hp == 100.));
        assert!(
            s.players.iter().all(|p| p.weapon == RIFLE && p.gun == RIFLE && p.ammo[RIFLE] == 60 && p.ammo[1] == 12 && p.owned.iter().filter(|held| **held).count() == 2),
            "everybody has the round's rifle and pistol again"
        );
        assert!((pos(&s, CRATE)[0] + 1.75).hypot(pos(&s, CRATE)[2] - 2.75) < 1e-3);
        assert_eq!(bacon(&s).p, [0., DECK, 3.1]);
        // Both teams at once.
        hush(&mut s);
        run(&mut s, 5.1);
        assert_eq!(s.stage, Stage::Fight);
        for id in 1..=6 {
            s.wound(id, 1000.);
        }
        s.tick(DT);
        let fort = s.fort.as_ref().unwrap();
        assert_eq!((s.stage, fort.winner, fort.scores), (Stage::RoundOver, Some(-1), [1, 0]));
        assert!(s.alert.contains("NO POINT"), "{}", s.alert);
    }

    #[test]
    fn the_first_team_to_three_rounds_wins_the_match_and_then_a_new_match_begins() {
        fn capture(s: &mut Feast, t: u8) {
            hush(s);
            run(s, 5.1);
            assert_eq!(s.stage, Stage::Fight);
            let id = team(s, t)[0];
            up(s, id, 0., 3.1);
            s.tick(DT);
            at(s, id, if t == 1 { 2.75 } else { -2.75 }, 3.1);
            s.tick(DT);
            assert_eq!(s.stage, Stage::RoundOver);
        }
        let mut s = fort(5.);
        capture(&mut s, 0);
        run(&mut s, INTERMISSION + 0.1);
        capture(&mut s, 1);
        run(&mut s, INTERMISSION + 0.1);
        capture(&mut s, 0);
        assert_eq!((s.fort.as_ref().unwrap().scores, s.fort.as_ref().unwrap().champion), ([2, 1], None));
        run(&mut s, INTERMISSION + 0.1);
        capture(&mut s, 0);
        assert_eq!((s.fort.as_ref().unwrap().scores, s.fort.as_ref().unwrap().champion, s.round), ([3, 1], Some(0), 4));
        assert!(s.alert.contains("THE MUSTARD EATERS WIN THE MATCH, 3 ROUNDS TO 1."), "{}", s.alert);
        assert_eq!(s.fort_state().unwrap().champion, Some(0));
        run(&mut s, INTERMISSION + 0.1);
        let fort = s.fort.as_ref().unwrap();
        assert_eq!((s.stage, fort.scores, fort.match_no, fort.champion, s.round), (Stage::Build, [0, 0], 2, None, 5));
    }

    #[test]
    fn a_stalemate_lets_the_cats_out_of_the_microwave_two_after_45_quiet_seconds_two_more_each_time_and_a_death_or_the_bacon_being_taken_starts_the_clock_again() {
        let mut s = fight();
        for p in &mut s.players {
            p.hp = 1e6;
        }
        at(&mut s, 1, -0.7, 2.6);
        let alarms = |s: &Feast| s.fort.as_ref().unwrap().alarms;
        run(&mut s, STALE - 1.);
        assert_eq!((alarms(&s), s.wave_spawned + s.pending()), (0, 0));
        assert!((s.fort_state().unwrap().stale - 1.).abs() < 0.2);
        run(&mut s, 1.1);
        assert_eq!((alarms(&s), s.wave_spawned + s.pending()), (1, PACK));
        assert!(s.alert.contains("STALEMATE. THE MICROWAVE DINGS: 2 ZOMBIE CATS."), "{}", s.alert);
        // The cats form and go for whoever is nearest, of either team.
        run(&mut s, 12.);
        assert_eq!(s.enemies.len() as u32, PACK);
        assert!(s.enemies.iter().all(|e| e.forming <= 0.));
        let hp: Vec<f32> = s.players.iter().map(|p| p.hp).collect();
        run(&mut s, 20.);
        assert!(s.players.iter().zip(&hp).any(|(p, hp)| p.hp < *hp), "somebody is bitten");
        run(&mut s, STALE - 32. + 0.1);
        assert_eq!((alarms(&s), s.wave_spawned + s.pending()), (2, PACK + PACK + GROWTH));
        // Taking the bacon is something happening; so is a death.
        run(&mut s, STALE - 2.);
        up(&mut s, 1, 0., 3.1);
        s.tick(DT);
        assert_eq!(bacon(&s).holder, 1);
        run(&mut s, 3.);
        assert_eq!(alarms(&s), 2);
        run(&mut s, STALE - 5.);
        s.hurt(4, f32::INFINITY);
        run(&mut s, 6.);
        assert_eq!(alarms(&s), 2);
        run(&mut s, STALE - 5.9);
        assert_eq!(alarms(&s), 3);
        // It never lets out more than PACK_MAX at once, and a new round starts the count again.
        let now = s.elapsed;
        {
            let fort = s.fort.as_mut().unwrap();
            (fort.alarms, fort.quiet) = (20, now - STALE);
        }
        let before = s.wave_spawned + s.pending();
        s.tick(DT);
        assert_eq!(s.wave_spawned + s.pending() - before, PACK_MAX);
        for id in team(&s, 1) {
            s.hurt(id, f32::INFINITY);
        }
        run(&mut s, INTERMISSION + 0.2);
        assert_eq!((s.stage, alarms(&s), s.enemies.len(), s.emergences.len()), (Stage::Build, 0, 0, 0));
    }

    #[test]
    fn there_is_no_hunger_no_eating_and_no_pests_in_a_forts_round() {
        let mut s = fight();
        run(&mut s, 12.);
        assert!(s.players.iter().all(|p| p.hp == 100.));
        assert!(s.pests.is_empty() && s.turrets.is_empty());
        s.wound(2, 1000.);
        let [x, _, z] = who(&s, 2).p;
        at(&mut s, 1, x, z);
        assert!(!s.eat(1) && !who(&s, 2).consumed);
    }

    /// On the old long fort, whose stock is chairs, crates, pillows and napkins (no boards, bread bins or rolls), the bots
    /// still build their forts to a plan, of what there is (`blueprint::Class::kinds`), each side on its own side,
    /// nothing in a base, and with some of it on top of the rest.
    #[test]
    fn bots_build_their_fort_to_a_plan_with_whatever_kinds_the_map_has_each_side_on_its_own_side() {
        let mut s = Feast::with_map(map(), true);
        s.set_build(60.);
        run(&mut s, 59.);
        assert_eq!(s.stage, Stage::Build);
        let fort = s.level.fort.clone().unwrap();
        for t in 0..2 {
            let plan = s.plan_of(t).expect("a plan");
            let own: Vec<usize> = (0..s.props.len()).filter(|i| (*i >= KETCHUP) == (t == 1)).collect();
            let placed: Vec<usize> = plan.places.iter().filter_map(|p| p.prop).collect();
            assert!(placed.len() >= 8, "team {t} built {} places", placed.len());
            assert!(placed.iter().all(|i| own.contains(i)), "with its own pieces");
            assert!(own.iter().all(|i| s.level.side(s.props.position(*i)[0], s.props.position(*i)[2]) == fort.sides[t]), "everything stayed on its side");
            assert!(plan.places.iter().any(|p| p.on.is_some() && p.prop.is_some()) || !s.drapes.is_empty(), "some of it is on top of the rest");
        }
        for i in 0..s.props.len() {
            let e = s.props.extent(i);
            assert!(fort.bases.iter().all(|r| e[2] <= r[0] || e[0] >= r[2] || e[3] <= r[1] || e[1] >= r[3]), "{i} in a base: {e:?}");
        }
        assert!(s.players.iter().all(|p| p.hp == 100. && s.level.side(p.p[0] as f64, p.p[2] as f64) == fort.sides[p.team as usize]));
    }


    /// On a bundled Forts map, its forts built in full: rounds are won by the bacon and by wipes, and somebody wins three.
    /// (On the old long fort with a 30-second build the bacon decides every round now that the bots go in and out of
    /// their forts by the gates rather than shoving at their own walls.)
    #[test]
    fn bots_play_a_match_out_rounds_are_won_by_the_bacon_and_by_wipes_and_somebody_wins_three() {
        let mut s = Feast::with_map(Map::parse(&read("assets/maps/jungle-fort.json")).unwrap(), true);
        let (mut reasons, mut matches, mut was) = (HashSet::new(), 0, Stage::Build);
        for _ in 0..30 * 60 * 45 {
            if matches >= 2 && reasons.len() >= 2 {
                break;
            }
            s.tick(DT);
            if s.stage == Stage::RoundOver && was != Stage::RoundOver {
                let fort = s.fort.as_ref().unwrap();
                reasons.insert(fort.reason.clone());
                if fort.champion.is_some() {
                    matches += 1;
                }
            }
            was = s.stage;
        }
        assert!(matches >= 2, "two matches were played out");
        assert!(reasons.contains("capture") && reasons.contains("wipe"), "rounds ended by {reasons:?}");
    }

    #[test]
    fn a_step_of_a_forts_fight_with_a_wall_on_each_side_is_well_under_2_ms() {
        let mut s = Feast::with_map(map(), true);
        s.set_build(30.);
        run(&mut s, 31.);
        assert_eq!(s.stage, Stage::Fight);
        let mut times = vec![];
        for _ in 0..300 {
            let from = std::time::Instant::now();
            s.tick(DT);
            times.push(from.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(f64::total_cmp);
        let (quick, median) = (times[75], times[150]);
        println!("a step of a Forts fight: quickest quarter {quick:.3} ms, median {median:.3} ms");
        let budget = if cfg!(debug_assertions) { 40. } else { 1.5 } * crate::timing_slack();
        assert!(median < budget, "quickest quarter {quick:.3} ms, median {median:.3} ms");
    }

    /// The bundled Forts maps' build stock is the kitchen's, at its true sizes beside a 15 cm hotdog, and set out tidily:
    /// 40 to 70 loose objects a map, nothing a kitchen counter does not have (no pillows, chairs or crates), five or six
    /// cutting boards, four to six rolls of paper towels, about six salt and pepper shakers, and nothing in a base. It
    /// all starts at rest (the piles locked) and stays where the map puts it; and a step stays under budget with all of it
    /// out and the bots at work, and with five pieces taken off the tops of the piles and thrown every 20 steps.
    #[test]
    fn the_bundled_forts_maps_set_out_the_kitchens_pieces_tidily_and_a_step_stays_under_budget() {
        // (A cutting board tumbling over a pile costs more than the pillows that were thrown about here once: 2 ms then.)
        let (budget, busy) = if cfg!(debug_assertions) { (40., 50.) } else { (1.5, 2.) };
        let (budget, busy) = (budget * crate::timing_slack(), busy * crate::timing_slack());
        for name in ["pillow-fort", "the-whole-kitchen-forts", "jungle-fort", "moonlit-fort"] {
            let map = Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap();
            let count = |kind: &str| map.loose.iter().filter(|l| l.kind == kind).count();
            assert!((40..=70).contains(&map.loose.len()), "{name}: {} loose objects", map.loose.len());
            for kind in ["pillow", "chair", "crate", "bowl", "tray"] {
                assert_eq!(count(kind), 0, "{name}: no {kind}s");
            }
            assert!((5..=6).contains(&count("board")) && (4..=6).contains(&count("roll")) && (5..=8).contains(&(count("salt") + count("pepper"))), "{name}");
            assert!(count("napkin") >= 2 && count("cereal") >= 4 && count("plate") >= 4, "{name}");
            let mut s = Feast::with_map(map, false);
            s.set_build(600.);
            let restless: Vec<String> = (0..s.props.len()).filter(|i| s.props.state(*i) == State::Awake).map(|i| format!("{i} {} at {:?}", crate::map::LOOSE[s.props.body(i).shape], s.props.position(i))).collect();
            assert!(restless.is_empty(), "{name}: everything starts at rest: {restless:?} awake");
            let put = (0..s.props.len()).map(|i| s.props.position(i)).collect::<Vec<_>>();
            let bases = s.level.fort.as_ref().unwrap().bases;
            // (Left alone: the bots stand by while it settles, and are back at work for the timing.)
            let bots: Vec<bool> = s.players.iter().map(|p| p.bot).collect();
            for p in &mut s.players {
                p.bot = false;
            }
            run(&mut s, 2.);
            for i in 0..s.props.len() {
                let e = s.props.extent(i);
                assert!(bases.iter().all(|r| e[2] <= r[0] || e[0] >= r[2] || e[3] <= r[1] || e[1] >= r[3]), "{name}: {i} in a base: {e:?}");
                let (a, b) = (put[i], s.props.position(i));
                assert!((a[0] - b[0]).hypot(a[2] - b[2]) < 0.002 && (a[1] - b[1]).abs() < 0.002, "{name}: {i} ({}) moved from {a:?} to {b:?}", crate::map::LOOSE[s.props.body(i).shape]);
            }
            for (p, bot) in s.players.iter_mut().zip(bots) {
                p.bot = bot;
            }
            let time = |s: &mut Feast| {
                let from = std::time::Instant::now();
                s.tick(DT);
                from.elapsed().as_secs_f64() * 1e3
            };
            let mut rest: Vec<f64> = (0..150).map(|_| time(&mut s)).collect();
            // The tops of the piles (of plates, boards, tins and napkins), five of them thrown every 20 steps.
            let tops: Vec<usize> = (0..s.props.len())
                .filter(|i| s.props.state(*i) == State::Locked)
                .filter(|i| {
                    let p = s.props.position(*i);
                    !(0..s.props.len()).any(|j| j != *i && {
                        let q = s.props.position(j);
                        (q[0] - p[0]).abs() < 0.01 && (q[2] - p[2]).abs() < 0.01 && q[1] > p[1] + 0.01
                    })
                })
                .collect();
            assert!(tops.len() >= 6, "{name}: {} piles", tops.len());
            let mut moving = vec![];
            for k in 0..150 {
                if k % 20 == 0 {
                    for j in 0..5 {
                        s.props.set_velocity(tops[(k / 20 * 5 + j) % tops.len()], [0.3, 0.6, 0.2], [0.; 3]);
                    }
                }
                moving.push(time(&mut s));
            }
            rest.sort_by(f64::total_cmp);
            moving.sort_by(f64::total_cmp);
            println!("{name}: {} loose objects; a step at rest {:.3} ms, with pieces thrown {:.3} ms (medians)", s.props.len(), rest[75], moving[75]);
            assert!(rest[75] < budget && moving[75] < busy, "{name}: {:.3} and {:.3} ms", rest[75], moving[75]);
        }
    }

    /// Draping in Forts: a narrow lane of a map (0.4 m across), where four crates side by side close it and three
    /// leave a way past.
    fn lane(crates: usize) -> Feast {
        let props: Vec<String> = [0.05, 0.15, 0.25, 0.35].iter().take(crates).map(|z| format!(r#"{{"kind": "crate", "p": [-0.8, {z}], "base": 0, "yaw": 0}}"#)).collect();
        let map = Map::parse(&format!(
            r#"{{"format": "hotdog-map", "version": 1, "name": "Lane", "mode": "forts", "build": 60, "bounds": [-1.5, 0, 1.5, 0.4], "nav": {{"cell": 0.04, "clearance": 0.035}},
            "starts": [{{"p": [-1.35, 0.2], "yaw": -1.5708, "team": "mustard"}}, {{"p": [1.35, 0.2], "yaw": 1.5708, "team": "ketchup"}}], "spawns": [{{"p": [0.6, 0.2]}}],
            "props": [{}, {{"kind": "napkin", "p": [-0.45, 0.2], "base": 0, "yaw": 0}}],
            "zones": [{{"kind": "base", "team": "mustard", "rect": [-1.5, 0, -1.2, 0.4]}}, {{"kind": "base", "team": "ketchup", "rect": [1.2, 0, 1.5, 0.4]}},
            {{"kind": "field", "rect": [-0.04, 0, 0.04, 0.4]}}, {{"kind": "bacon", "p": [0, 0.2]}}]}}"#,
            props.join(", ")
        ))
        .unwrap();
        let mut s = Feast::with_map(map, false);
        s.set_build(60.);
        hush(&mut s);
        run(&mut s, 1.);
        s
    }
    /// Hotdog 1 carries the lane's napkin onto the crates from the field's side.
    fn drape(s: &mut Feast) -> usize {
        let id = s.props.len() - 1;
        at(s, 1, -0.45, 0.2);
        let d = look(s, 1, [0., -1., 0.05]);
        s.tick(DT);
        assert!(s.grab(1, d));
        assert_eq!(who(s, 1).carry, id as i32);
        at(s, 1, -0.8 + OUT[3] as f32, 0.2);
        let d = look(s, 1, [-1., 0., 0.]);
        s.tick(DT);
        assert!(s.put(1, d));
        run(s, 3.);
        id
    }
    #[test]
    fn a_draped_wall_comes_off_at_any_time_in_the_build_by_its_own_side_in_the_fight_by_either_side_a_bot_does_not_grab_one_and_a_drape_that_would_seal_a_base_off_from_the_bacon_is_refused() {
        let mut s = lane(3);
        let id = drape(&mut s);
        assert_eq!(s.drapes.len(), 1);
        assert_eq!(s.drapes[0].props, [0, 1, 2]);
        // During the build its own side pulls it off, and puts it back.
        at(&mut s, 1, -0.6, 0.2);
        let d = look(&mut s, 1, [-1., -0.6, 0.]);
        s.tick(DT);
        assert!(s.grab(1, d));
        assert_eq!((who(&s, 1).carry, s.drapes.len()), (id as i32, 0));
        at(&mut s, 1, -0.8 + OUT[3] as f32, 0.2);
        let d = look(&mut s, 1, [-1., 0., 0.]);
        s.tick(DT);
        assert!(s.put(1, d));
        run(&mut s, 3.);
        assert_eq!(s.drapes.len(), 1);
        // A bot does not grab one (it pulls one only when walls have cut it off: see `breach`).
        s.player_mut(1).unwrap().bot = true;
        at(&mut s, 1, -0.6, 0.2);
        let d = look(&mut s, 1, [-1., -0.6, 0.]);
        s.tick(DT);
        assert!(!s.grab(1, d));
        s.player_mut(1).unwrap().bot = false;
        // Once the field is down a rival can pull it off too: a Ketchup Connoisseur comes over and takes the napkin, and the
        // way is open.
        s.drop_field();
        assert_eq!(s.stage, Stage::Fight);
        let r = team(&s, 1)[0];
        at(&mut s, 1, -1.35, 0.2);
        at(&mut s, r, -0.6, 0.2);
        let d = look(&mut s, r, [-1., -0.6, 0.]);
        s.tick(DT);
        assert!(s.grab(r, d));
        assert_eq!((who(&s, r).carry, s.drapes.len()), (id as i32, 0));
        assert!(s.level.clear(-0.8, 0.1) && !s.level.path([-1.3, 0.2], [0., 0.2]).is_empty(), "the wall is gone from the level");
        // It puts it back over the crates (it drapes again in the fight), and its own side pulls it off again.
        at(&mut s, r, -0.8 + OUT[3] as f32, 0.2);
        let d = look(&mut s, r, [-1., 0., 0.]);
        s.tick(DT);
        assert!(s.put(r, d));
        run(&mut s, 3.);
        assert_eq!(s.drapes.len(), 1);
        at(&mut s, r, 1., 0.2);
        at(&mut s, 1, -0.6, 0.2);
        let d = look(&mut s, 1, [-1., -0.6, 0.]);
        s.tick(DT);
        assert!(s.grab(1, d));
        assert_eq!((who(&s, 1).carry, s.drapes.len()), (id as i32, 0));
        // The next round puts everything back.
        for id in team(&s, 1) {
            s.wound(id, 1000.);
        }
        run(&mut s, INTERMISSION + 0.2);
        assert_eq!((s.stage, s.drapes.len()), (Stage::Build, 0));
        assert!(s.level.clear(-0.8, 0.38) && (0..s.props.len()).all(|id| matches!(s.props.state(id), State::Awake | State::Asleep)));
        // Over four crates the wall would close the lane: the mustard base would have no way to the bacon, so the
        // napkin stays a loose napkin on top.
        let mut s = lane(4);
        let id = drape(&mut s);
        assert_eq!((s.drapes.len(), s.props.state(id)), (0, State::Locked));
        assert!(s.props.position(id)[1] > crate::map::FLOOR + 0.09, "it lies on the crates");
        assert!(s.level.clear(-0.8, 0.2), "and the level has no wall");
    }
    /// Asking whether a drape would be refused (`Feast::refusal`, which builds a navigation grid with the new wall to
    /// see that both bases still reach the bacon) is cheap enough for the client to ask it every frame that it shows
    /// a napkin about to drape, on every bundled Forts map with all its stock out and a hotdog standing in the wall's
    /// way.
    #[test]
    fn asking_whether_a_drape_would_be_refused_is_cheap_enough_for_every_frame() {
        // A millisecond, a sixteenth of a frame at 60 a second. Alone the query takes 0.13-0.32 ms; with the whole
        // suite running beside it on every core it takes about twice that, so half a millisecond was too tight.
        let budget = if cfg!(debug_assertions) { 20. } else { 1. } * crate::timing_slack();
        for name in ["pillow-fort", "the-whole-kitchen-forts", "jungle-fort", "moonlit-fort"] {
            let mut s = Feast::with_map(Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap(), false);
            s.set_build(600.);
            run(&mut s, 0.5);
            // A wall 0.3 m by 0.2 m round hotdog 1, as one over a pile beside it would be.
            let p = who(&s, 1).p.map(|v| v as f64);
            let canopy = crate::drape::Canopy { props: vec![], rect: [p[0] - 0.15, p[2] - 0.1, p[0] + 0.15, p[2] + 0.1], top: p[1] + 0.1 };
            let mut took: Vec<f64> = (0..40)
                .map(|_| {
                    let from = std::time::Instant::now();
                    std::hint::black_box(s.refusal(&canopy, &s.bodies));
                    from.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            took.sort_by(f64::total_cmp);
            println!("{name}: whether a drape would be refused, {:.3} ms (median), {:.3} ms at most", took[20], took[39]);
            // Judged by the fastest quarter: the suite runs its tests side by side, and a busy machine slows the
            // median of a sub-millisecond measure without the query itself costing any more.
            assert!(took[10] < budget, "{name}: {:.3} ms (fastest quarter), {:.3} ms (median)", took[10], took[20]);
        }
    }
    /// The placement preview of a napkin about to be put over the lane's crates says what the landing then does:
    /// over three crates it drapes and the drape stands; over four the wall would close the lane, and `refusal` says
    /// so (Sealed) before the napkin is put down, which then stays a loose napkin.
    #[test]
    fn the_preview_of_a_drape_that_would_seal_a_base_says_it_is_refused_and_the_landing_agrees() {
        for (crates, refused) in [(3, None), (4, Some(crate::drape::Refusal::Sealed))] {
            let mut s = lane(crates);
            let id = s.props.len() - 1;
            at(&mut s, 1, -0.45, 0.2);
            let d = look(&mut s, 1, [0., -1., 0.05]);
            s.tick(DT);
            assert!(s.grab(1, d));
            at(&mut s, 1, -0.8 + OUT[3] as f32, 0.2);
            let d = look(&mut s, 1, [-1., 0., 0.]);
            s.tick(DT);
            let spot = s.placement(1).unwrap();
            let canopy = spot.drape.clone().expect("over the crates it would drape");
            assert_eq!(canopy.props.len(), crates);
            assert_eq!(s.refusal(&canopy, &s.bodies), refused, "{crates} crates");
            assert!(s.put(1, d));
            run(&mut s, 3.);
            assert_eq!(s.drapes.len(), if refused.is_none() { 1 } else { 0 }, "{crates} crates");
            assert_eq!(s.drapes.first().map(|d| d.props.clone()), refused.is_none().then_some(canopy.props), "{crates} crates");
            assert!(refused.is_none() || s.props.state(id) == State::Locked);
        }
    }
    /// A counter with a pocket on the mustard side: two low blocks 0.5 m apart, open towards the bacon, and three crates
    /// across its mouth, a gap between each two. A napkin over them seals the pocket (a draped wall stands over the
    /// whole outline of what it covers), which is allowed: neither base is cut off from the bacon.
    fn pocket() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Pocket", "mode": "forts", "build": 60, "bounds": [-1.5, 0, 1.5, 1.2], "nav": {"cell": 0.04, "clearance": 0.035},
            "starts": [{"p": [-1.35, 0.2], "yaw": -1.5708, "team": "mustard"}, {"p": [1.35, 0.2], "yaw": 1.5708, "team": "ketchup"}], "spawns": [{"p": [1, 1]}],
            "blocks": [{"rect": [-1.05, 0.5, -1, 1.2], "height": 0.08, "style": "wood"}, {"rect": [-0.5, 0.5, -0.45, 1.2], "height": 0.08, "style": "wood"}],
            "props": [{"kind": "crate", "p": [-0.95, 0.55], "base": 0, "yaw": 0}, {"kind": "crate", "p": [-0.75, 0.55], "base": 0, "yaw": 0}, {"kind": "crate", "p": [-0.55, 0.55], "base": 0, "yaw": 0},
            {"kind": "napkin", "p": [-0.75, 0.2], "base": 0, "yaw": 0}],
            "zones": [{"kind": "base", "team": "mustard", "rect": [-1.5, 0, -1.2, 1.2]}, {"kind": "base", "team": "ketchup", "rect": [1.2, 0, 1.5, 1.2]},
            {"kind": "field", "rect": [-0.04, 0, 0.04, 1.2]}, {"kind": "bacon", "p": [0, 0.3]}]}"#,
        )
        .unwrap();
        let mut s = Feast::with_map(map, false);
        s.set_build(60.);
        hush(&mut s);
        for i in 1..s.players.len() {
            let p = &mut s.players[i];
            p.p = [if p.team == 1 { 1.35 } else { -1.4 }, F, 0.1 + p.id as f32 * 0.1];
        }
        run(&mut s, 1.);
        s
    }
    #[test]
    fn a_bot_that_draped_walls_have_cut_off_in_the_fight_pulls_the_wall_down_lays_the_napkin_aside_and_goes_on_one_that_can_go_round_does_not() {
        let mut s = pocket();
        // Hotdog 1 lays the napkin over the crates from the south, and a team mate stands in the pocket behind them.
        let id = s.props.len() - 1;
        let mate = team(&s, 0).into_iter().find(|q| *q != 1).unwrap();
        at(&mut s, mate, -0.75, 0.95);
        at(&mut s, 1, -0.75, 0.42);
        let d = look(&mut s, 1, [0., -1., -0.05]);
        s.tick(DT);
        assert!(s.grab(1, d));
        at(&mut s, 1, -0.75, 0.55 - OUT[3] as f32);
        let d = look(&mut s, 1, [0., 0., 1.]);
        s.tick(DT);
        assert!(s.put(1, d));
        run(&mut s, 3.);
        assert_eq!(s.drapes.len(), 1, "the pocket is sealed");
        assert_eq!(s.drapes[0].props, [0, 1, 2]);
        assert!(!s.level.joined([-0.75, 0.95], [0., 0.3]));
        at(&mut s, 1, -1.35, 0.2);
        // In the fight it breaks out. (It has the most HP left of its team, so it is not the one that stands guard: it
        // makes for the bacon.)
        s.drop_field();
        s.player_mut(mate).unwrap().bot = true;
        for q in team(&s, 0).into_iter().filter(|q| *q != mate) {
            s.player_mut(q).unwrap().hp = 60.;
        }
        let mut pulled = None;
        for i in 0..30 * 8 {
            s.tick(DT);
            if s.drapes.is_empty() {
                pulled = Some(i);
                break;
            }
        }
        assert!(pulled.is_some(), "the bot pulled the wall down (it stands at {:?})", who(&s, mate).p);
        assert_eq!(who(&s, mate).carry, -1, "and laid the napkin down");
        assert!(s.props.position(id)[2] > 0.62 && matches!(s.props.state(id), State::Awake | State::Asleep | State::Locked), "behind itself, in the pocket: {:?}", s.props.position(id));
        run(&mut s, 6.);
        // (Out by its mouth: at a walk it may be off round the pocket's side by now, for the bacon and home with it.)
        let out = who(&s, mate).p;
        assert!(out[2] < 0.5 || out[0] < -1.05 || out[0] > -0.45, "and it came out of the pocket: {out:?}");
        assert_eq!(s.drapes.len(), 0, "the napkin did not drape again");
        // A bot that can go round a wall leaves it standing: the lane of three crates leaves a way past.
        let mut s = lane(3);
        for i in 1..s.players.len() {
            s.players[i].p = [1.35, F, 0.2];
        }
        drape(&mut s);
        assert_eq!(s.drapes.len(), 1);
        s.drop_field();
        let mate = team(&s, 0).into_iter().find(|q| *q != 1).unwrap();
        at(&mut s, mate, -1.3, 0.3);
        at(&mut s, 1, -1.4, 0.1);
        s.player_mut(mate).unwrap().bot = true;
        for q in team(&s, 0).into_iter().filter(|q| *q != mate) {
            s.player_mut(q).unwrap().hp = 60.;
        }
        // (At a walk it may be at the bacon and home again by the end: how far it got is what tells.)
        let mut furthest = f32::MIN;
        for _ in 0..30 * 6 {
            s.tick(DT);
            furthest = furthest.max(who(&s, mate).p[0]);
        }
        assert_eq!(s.drapes.len(), 1, "the wall still stands");
        assert!(furthest > -0.7, "the bot went round it: as far as x = {furthest}");
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The lobby and the game's server-side parts, driven directly (tests/server.rs drives them over sockets). These are
//! the Node server's lobby.test.mjs and the parts of simulation.test.mjs and forts.test.mjs that were the server's own
//! (seats, the lobby's stages, commands, frames), and the game loop's (its steps, the frames each socket is sent
//! against what it acknowledged); kitchen-sim's own tests hold the rules of a round.
use crate::game::{BARE, COLOURS, COUNTDOWN, Game, OPENING, Phase, STEP, Settings};
use crate::lobby::{self, CAPACITY, Catalogue, HISTORY, Lobby, Options, QUIET, RATE, SIGNAL, scan};
use crate::outbox::Outbox;
use crate::hub::Hub;
use glam::{Vec2, Vec3};
use kitchen_sim::cards;
use kitchen_sim::actor::WALK;
use kitchen_sim::feast::{CAT, FLOOR, Feast, NAMES, RIFLE, Stage};
use kitchen_sim::net::{Command, button};
use kitchen_sim::forts::{self, INTERMISSION};
use kitchen_sim::map::{Map, TEAM_NAMES};
use kitchen_sim::rules::Rules;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// Steps a second.
const HZ: f64 = 1. / STEP as f64;
fn read_map(path: &str) -> Map {
    Map::parse(&std::fs::read_to_string(format!("{ROOT}/{path}")).unwrap()).unwrap()
}
fn maps_folder() -> PathBuf {
    Path::new(ROOT).join("assets/maps")
}

// ----- A lobby over a real game, with a clock the test turns and tokens that can be read.

type Inbox = Arc<Mutex<Vec<String>>>;
struct Page {
    inbox: Inbox,
    member: Option<u32>,
    player: Option<u32>,
}
impl Page {
    fn member(&self) -> u32 {
        self.member.expect("a member")
    }
    fn messages(&self) -> Vec<Value> {
        self.inbox.lock().unwrap().iter().map(|m| serde_json::from_str(m).unwrap()).collect()
    }
    fn last(&self, kind: &str) -> Option<Value> {
        self.messages().into_iter().rev().find(|m| m["type"] == kind)
    }
    fn all(&self, kind: &str) -> Vec<Value> {
        self.messages().into_iter().filter(|m| m["type"] == kind).collect()
    }
    fn says(&self, text: &str) -> bool {
        self.all("system").iter().any(|m| m["text"] == text)
    }
}
struct Table {
    lobby: Lobby,
    time: Arc<AtomicU64>,
}
impl Table {
    fn new(settings: Settings, map: Option<Map>, change: impl FnOnce(&mut Options)) -> Table {
        let time = Arc::new(AtomicU64::new(0f64.to_bits()));
        let clock = time.clone();
        let tokens = Arc::new(AtomicU64::new(0));
        let mut options = Options::new(maps_folder());
        options.now = Box::new(move || f64::from_bits(clock.load(Ordering::Relaxed)));
        options.token = Box::new(move || format!("token-{}", tokens.fetch_add(1, Ordering::Relaxed) + 1));
        change(&mut options);
        let game = Game::new(map.unwrap_or_else(|| read_map("assets/maps/counter-siege.json")), &settings);
        Table { lobby: Lobby::new(game, settings, options), time }
    }
    fn plain() -> Table {
        Table::new(Settings::default(), None, |_| {})
    }
    fn timed(countdown: f32, opening: f32) -> Table {
        Table::new(Settings { countdown, opening, ..Settings::default() }, None, |_| {})
    }
    fn advance(&self, ms: f64) {
        let now = f64::from_bits(self.time.load(Ordering::Relaxed)) + ms;
        self.time.store(now.to_bits(), Ordering::Relaxed);
    }
    /// One browser: its lobby socket, then its game socket.
    fn page(&mut self, hello: Value) -> Page {
        let (outbox, inbox) = Outbox::memory();
        let member = self.lobby.enter(outbox, &hello);
        let mut player = None;
        if let Some(member) = member {
            player = self.lobby.game.join();
            if let Some(p) = player {
                let token = Value::String(self.lobby.member(member).unwrap().token.clone());
                let claimed = self.lobby.claim(&token).unwrap();
                self.lobby.seat(claimed, p);
            }
        }
        Page { inbox, member, player }
    }
    fn run(&mut self, seconds: f64) {
        for _ in 0..(seconds * HZ).round() as usize {
            self.advance(1000. / HZ);
            self.lobby.game.tick(STEP);
            self.lobby.tick();
        }
    }
    fn hotdog(&self, page: &Page) -> &kitchen_sim::feast::Player {
        self.lobby.game.player(self.lobby.player(page.member()).unwrap()).unwrap()
    }
    fn ready(&mut self, page: &Page, flag: bool) -> bool {
        self.lobby.ready(page.member(), flag)
    }
    fn handle(&mut self, page: &Page, message: Value) {
        self.lobby.handle(page.member(), &message)
    }
    /// The lobby ends the round it is in: everyone else dies (a siege), or the Mustard Eaters win the match (Forts);
    /// then the lobby again.
    fn finish(&mut self, winner: Option<u32>) {
        // (A round that has not begun yet, its start sequence still playing, begins first.)
        while self.lobby.game.phase != Phase::Round {
            self.run(0.1);
        }
        if self.lobby.game.forts() {
            self.lobby.game.feast.fort.as_mut().unwrap().scores = [2, 0];
            self.lobby.game.feast.win(0, "wipe", None);
            self.run((INTERMISSION + 0.3) as f64);
        } else {
            let feast = &mut self.lobby.game.feast;
            for p in feast.players.iter_mut().filter(|p| Some(p.id) != winner) {
                (p.hp, p.alive) = (0., false);
            }
            feast.stage = Stage::FreeForAll;
            self.run(8.4);
        }
        assert_eq!(self.lobby.game.stage(), "Lobby");
    }
}
fn snapshot(lobby: &mut Lobby) -> Value {
    serde_json::from_str(&lobby.snapshot()).unwrap()
}

#[test]
fn a_taken_name_is_suffixed_and_the_bots_names_are_taken_too_and_no_name_at_all_is_hotdog() {
    let mut t = Table::plain();
    let mut pages: Vec<Page> = ["Dill", "dill", " DILL ", "Frankie"].iter().map(|name| t.page(json!({"name": name}))).collect();
    pages.push(t.page(json!({})));
    let names: Vec<String> = pages.iter().map(|p| t.lobby.member(p.member()).unwrap().name.clone()).collect();
    assert_eq!(names, ["Dill", "dill 2", "DILL 3", "Frankie 2", "HOTDOG"]);
}
#[test]
fn a_players_colour_is_one_of_five_and_nothing_else() {
    assert_eq!(COLOURS, ["red", "blue", "green", "yellow", "black"]);
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann", "color": "black"}));
    let b = t.page(json!({"name": "Ben", "color": "<script>"}));
    assert_eq!(t.lobby.member(a.member()).unwrap().color, "black");
    assert!(COLOURS.contains(&t.lobby.member(b.member()).unwrap().color.as_str()), "an invalid choice is replaced by a colour of the five");
    for bad in [json!("pink"), json!("RED"), json!(" red"), json!(0), json!(null), json!(["red"])] {
        t.handle(&b, json!({"type": "profile", "color": bad}));
        assert_ne!(t.lobby.member(b.member()).unwrap().color, "pink");
        assert!(COLOURS.contains(&t.lobby.member(b.member()).unwrap().color.as_str()));
    }
    t.handle(&b, json!({"type": "profile", "color": "green"}));
    assert_eq!(t.lobby.member(b.member()).unwrap().color, "green");
    // There is one colour: the two there were before, lips and face, are not taken.
    t.handle(&b, json!({"type": "profile", "lips": "red", "face": "red"}));
    assert_eq!(t.lobby.member(b.member()).unwrap().color, "green");
    // The game shows it: in the snapshot beside the name, and bots keep the built-in name and are bare.
    let seen = snapshot(&mut t.lobby)["players"].as_array().unwrap().clone();
    let humans: Vec<(String, String)> = seen.iter().filter(|p| p["human"] == true).map(|p| (p["name"].as_str().unwrap().into(), p["color"].as_str().unwrap().into())).collect();
    assert_eq!(humans, [("Ann".to_string(), "black".to_string()), ("Ben".into(), "green".into())]);
    let bots: Vec<(String, String, bool)> = seen.iter().filter(|p| p["human"] == false).map(|p| (p["name"].as_str().unwrap().into(), p["color"].as_str().unwrap().into(), p["bot"] == true)).collect();
    assert_eq!(bots, NAMES[2..].iter().map(|n| (n.to_string(), BARE.to_string(), true)).collect::<Vec<_>>());
    assert!(seen.iter().all(|p| p.get("lips").is_none() && p.get("face").is_none()));
}
#[test]
fn a_profile_lives_on_the_server_reaches_everyone_and_stays_with_its_player_across_rounds() {
    let mut t = Table::timed(0.1, 0.1);
    let a = t.page(json!({"name": "Ann", "color": "blue"}));
    let b = t.page(json!({"name": "Ben"}));
    t.handle(&a, json!({"type": "profile", "name": "  Annie  "}));
    t.run(0.1);
    assert_eq!(t.lobby.member(a.member()).unwrap().name, "Annie");
    assert!(b.says("Ann is now Annie"));
    let members: Vec<Value> = b.last("roster").unwrap()["members"].as_array().unwrap().iter().map(|m| json!([m["name"], m["color"]])).collect();
    assert_eq!(members, [json!(["Annie", "blue"]), json!(["Ben", t.lobby.member(b.member()).unwrap().color])]);
    t.handle(&a, json!({"type": "profile", "name": "ben"}));
    assert_eq!(t.lobby.member(a.member()).unwrap().name, "ben 2", "a change of name is made unique too");
    t.handle(&a, json!({"type": "profile", "name": "Annie"}));
    for page in [&a, &b] {
        t.handle(page, json!({"type": "ready", "ready": true}));
    }
    t.run(0.5);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    assert_eq!(t.hotdog(&a).name, "Annie");
    let mine = a.player.unwrap();
    t.finish(Some(mine));
    // (The round's message used the chosen name.)
    assert!(t.lobby.game.feast.alert.starts_with("IN THE FRIDGE"));
    assert_eq!(t.hotdog(&a).name, "Annie");
    assert_eq!(snapshot(&mut t.lobby)["players"][0]["color"], "blue");
}
#[test]
fn the_last_hotdog_standing_is_named_as_its_player_chose() {
    let mut t = Table::timed(0.1, 0.1);
    let a = t.page(json!({"name": "Annie"}));
    t.ready(&a, true);
    t.run(0.5);
    let feast = &mut t.lobby.game.feast;
    for p in feast.players.iter_mut().filter(|p| Some(p.id) != a.player) {
        (p.hp, p.alive) = (0., false);
    }
    feast.stage = Stage::FreeForAll;
    t.run(0.1);
    assert_eq!(t.lobby.game.feast.stage, Stage::RoundOver);
    assert_eq!(t.lobby.game.feast.alert, "Annie IS THE LAST HOTDOG STANDING", "the round's messages use the chosen name");
}
#[test]
fn everyone_ready_starts_the_countdown_it_stops_when_somebody_changes_their_mind_or_sits_down_then_the_fridge_opens_and_the_round_begins() {
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    assert_eq!(t.lobby.game.stage(), "Lobby");
    t.run(2.);
    assert_eq!((t.lobby.game.stage(), t.lobby.game.countdown), ("Lobby", None));
    assert_eq!(a.last("roster").unwrap()["countdown"], Value::Null);
    t.handle(&a, json!({"type": "ready", "ready": true}));
    t.run((COUNTDOWN + 1.) as f64);
    assert_eq!((t.lobby.game.stage(), t.lobby.game.countdown), ("Lobby", None), "one of two is not everyone");
    let ready: Vec<Value> = b.last("roster").unwrap()["members"].as_array().unwrap().iter().map(|m| m["ready"].clone()).collect();
    assert_eq!(ready, [json!(true), json!(false)]);
    assert!(b.says("Ann is ready"));
    t.handle(&b, json!({"type": "ready", "ready": true}));
    t.run(1.);
    let left = t.lobby.game.countdown.unwrap();
    assert!(left > 0. && left < COUNTDOWN);
    assert_eq!(a.last("roster").unwrap()["countdown"], json!(left.ceil() as i64));
    assert!(t.lobby.game.feast.alert.starts_with("THE FRIDGE OPENS IN "), "{}", t.lobby.game.feast.alert);
    // Changing one's mind stops it, and it starts again from the top.
    t.handle(&b, json!({"type": "ready", "ready": false}));
    t.run(0.1);
    assert_eq!(t.lobby.game.countdown, None);
    assert_eq!(a.last("roster").unwrap()["countdown"], Value::Null);
    t.run((COUNTDOWN + 1.) as f64);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    t.handle(&b, json!({"type": "ready", "ready": true}));
    t.run((COUNTDOWN - 0.5) as f64);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    // So does somebody new sitting down.
    let c = t.page(json!({"name": "Cy"}));
    t.run(0.1);
    assert_eq!(t.lobby.game.countdown, None);
    assert!(!t.hotdog(&c).ready);
    t.run((COUNTDOWN + 1.) as f64);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    t.handle(&c, json!({"type": "ready", "ready": true}));
    t.run((COUNTDOWN + 0.2) as f64);
    assert_eq!(t.lobby.game.stage(), "Opening");
    assert!(a.all("system").iter().any(|m| m["text"].as_str().unwrap().contains("the fridge opens")));
    assert_eq!(a.last("roster").unwrap()["stage"], "Opening");
    // Nothing stops the start sequence, and nothing on the counter moves until it is over.
    t.handle(&a, json!({"type": "ready", "ready": false}));
    let snap = snapshot(&mut t.lobby);
    let opening = snap["opening"].as_f64().unwrap();
    assert!(opening > 0. && opening < OPENING as f64 && snap["opening_for"] == json!(OPENING));
    let before: Vec<[f32; 3]> = t.lobby.game.feast.players.iter().map(|p| p.p).collect();
    t.lobby.game.commands(a.player.unwrap(), &[Command::new(1, Vec2::new(0., -1.), 0., 0., button::FIRE, 1, None)]);
    t.run((OPENING - 1.) as f64);
    assert_eq!(t.lobby.game.stage(), "Opening");
    assert_eq!(t.lobby.game.feast.players.iter().map(|p| p.p).collect::<Vec<_>>(), before);
    assert_eq!(t.hotdog(&a).ammo[1], 12);
    t.run(1.2);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    assert!(t.lobby.game.feast.elapsed < 0.5, "the warm-up begins when the sequence ends");
    assert_eq!(snapshot(&mut t.lobby)["countdown"], Value::Null);
    assert_eq!(t.lobby.game.feast.players.iter().filter(|p| p.bot).count(), 2, "bots fill the seats nobody took");
}
#[test]
fn one_player_alone_can_ready_up_and_play_with_four_bots() {
    let mut t = Table::timed(1., 1.);
    let a = t.page(json!({"name": "Solo"}));
    t.run(3.);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    t.ready(&a, true);
    t.run(2.2);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    assert_eq!(t.lobby.game.feast.players.iter().map(|p| p.bot).collect::<Vec<_>>(), [false, true, true, true, true]);
}
#[test]
fn a_finished_round_returns_everyone_to_the_lobby_unready_instead_of_starting_another() {
    let mut t = Table::timed(0.1, 0.1);
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    for page in [&a, &b] {
        t.ready(page, true);
    }
    t.run(0.5);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    let round = t.lobby.game.feast.round;
    let feast = &mut t.lobby.game.feast;
    for p in feast.players.iter_mut().filter(|p| Some(p.id) != b.player) {
        (p.hp, p.alive) = (0., false);
    }
    feast.stage = Stage::FreeForAll;
    t.run(0.1);
    assert_eq!(t.lobby.game.feast.stage, Stage::RoundOver);
    assert_eq!(t.lobby.game.feast.winner, b.player);
    t.run(7.);
    assert_eq!(t.lobby.game.feast.stage, Stage::RoundOver, "the winner is shown for eight seconds");
    t.run(1.5);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    assert!(t.lobby.game.feast.round > round);
    assert!(t.lobby.game.feast.players.iter().all(|p| p.alive && p.hp == 100. && !p.ready));
    assert!(a.says("Round over: back in the fridge"));
    assert_eq!(a.last("roster").unwrap()["members"].as_array().unwrap().iter().map(|m| m["ready"].clone()).collect::<Vec<_>>(), [json!(false), json!(false)]);
    t.run((COUNTDOWN + OPENING + 2.) as f64);
    assert_eq!(t.lobby.game.stage(), "Lobby", "nothing starts until they are ready again");
}
#[test]
fn somebody_who_arrives_during_a_round_waits_in_the_fridge_and_plays_the_next_one() {
    let mut t = Table::timed(0.1, 0.5);
    let a = t.page(json!({"name": "Ann"}));
    t.ready(&a, true);
    t.run(0.3);
    assert_eq!(t.lobby.game.stage(), "Opening");
    let late = t.page(json!({"name": "Late"}));
    t.run(0.6);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    let id = late.player.unwrap();
    assert!(t.lobby.game.waiting(id) && t.lobby.game.player(id).unwrap().bot && t.lobby.game.connected(id), "the bot goes on playing the seat");
    assert_eq!(t.lobby.game.player(id).unwrap().name, NAMES[id as usize - 1]);
    let seen = snapshot(&mut t.lobby);
    let me = seen["players"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(me["profile"], json!({"name": "Late", "color": t.lobby.member(late.member()).unwrap().color}));
    assert_eq!((me["waiting"].clone(), me["human"].clone(), me["color"].clone()), (json!(true), json!(true), json!("")));
    let roster = late.last("roster").unwrap();
    assert_eq!(roster["members"].as_array().unwrap().iter().find(|m| m["id"] == late.member()).unwrap()["waiting"], true);
    // It cannot ready up or steer, and is told why.
    t.ready(&late, true);
    assert!(!t.lobby.game.player(id).unwrap().ready);
    assert!(late.last("notice").unwrap()["text"].as_str().unwrap().contains("round is in progress"));
    let ammo = t.lobby.game.player(id).unwrap().ammo;
    t.lobby.game.commands(id, &[Command::new(1, Vec2::new(1., 0.), 0., 0., button::FIRE | button::STIM, 2, Some(0))]);
    t.lobby.game.tick(STEP);
    assert_eq!(t.lobby.game.applied(id), 1, "taken as read");
    let bot = t.lobby.game.player(id).unwrap();
    assert!(bot.stim == 0. && bot.ammo[2..] == ammo[2..], "none of it was done for the waiting player");
    // A second arrival waits too; the round ends; both are seated with their own names.
    let later = t.page(json!({"name": "Later"}));
    assert!(t.lobby.game.waiting(later.player.unwrap()));
    let feast = &mut t.lobby.game.feast;
    for p in feast.players.iter_mut().filter(|p| Some(p.id) != a.player) {
        (p.hp, p.alive) = (0., false);
    }
    feast.stage = Stage::FreeForAll;
    t.run(8.5);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    for (page, name) in [(&late, "Late"), (&later, "Later")] {
        let p = page.player.unwrap();
        let hotdog = t.lobby.game.player(p).unwrap();
        assert_eq!((t.lobby.game.waiting(p), hotdog.bot, hotdog.name.as_str(), hotdog.hp), (false, false, name, 100.));
    }
    // When the only seated players leave a round, those waiting are not made to sit through the bots' game.
    for page in [&a, &late, &later] {
        t.ready(page, true);
    }
    t.run(1.);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    let third = t.page(json!({"name": "Third"}));
    assert!(t.lobby.game.waiting(third.player.unwrap()));
    for page in [&a, &late, &later] {
        t.lobby.game.leave(page.player.unwrap());
        t.lobby.unseat(page.member());
        t.lobby.leave(page.member());
    }
    assert_eq!(t.lobby.game.stage(), "Lobby");
    assert!(!t.lobby.game.waiting(third.player.unwrap()));
    assert_eq!(t.lobby.game.player(third.player.unwrap()).unwrap().name, "Third");
}
#[test]
fn a_sixth_player_is_refused_in_the_lobby_and_at_the_game_socket() {
    let mut t = Table::plain();
    assert_eq!(CAPACITY, 5);
    let five: Vec<Page> = (1..=5).map(|i| t.page(json!({"name": format!("P{i}")}))).collect();
    assert!(five.iter().all(|p| p.member.is_some() && p.player.is_some()));
    let sixth = t.page(json!({"name": "P6"}));
    assert_eq!(sixth.member, None);
    assert_eq!(sixth.messages(), [json!({"type": "error", "message": "Kitchen full (5 hotdogs)"})]);
    assert_eq!(t.lobby.guest(&json!({"name": "Native"})), None, "and a client without a page");
    assert_eq!(t.lobby.game.join(), None);
    assert_eq!(t.lobby.members.len(), 5);
    // A place is free again once somebody has gone.
    t.lobby.game.leave(five[0].player.unwrap());
    t.lobby.unseat(five[0].member());
    t.lobby.leave(five[0].member());
    assert_eq!(t.lobby.members.len(), 4);
    let next = t.page(json!({"name": "P6"}));
    assert_eq!(next.player, five[0].player);
    assert_eq!(t.hotdog(&next).name, "P6");
}
#[test]
fn a_token_ties_a_page_to_its_hotdog_once() {
    let mut t = Table::plain();
    let (outbox, inbox) = Outbox::memory();
    let member = t.lobby.enter(outbox, &json!({"name": "Ann", "color": "blue"})).unwrap();
    let page = Page { inbox, member: Some(member), player: None };
    let welcome = page.last("welcome").unwrap();
    let token = t.lobby.member(member).unwrap().token.clone();
    assert_eq!((welcome["token"].as_str().unwrap(), welcome["id"].as_u64().unwrap()), (token.as_str(), member as u64));
    assert_eq!(welcome["colours"], json!(COLOURS));
    assert_eq!(welcome["limits"], json!({"name": 16, "chat": 200}));
    for bad in [json!("nonsense"), Value::Null, json!({})] {
        assert_eq!(t.lobby.claim(&bad), None);
    }
    // Before its game has joined a member is in the lobby without a hotdog: it cannot be ready.
    t.handle(&page, json!({"type": "ready", "ready": true}));
    assert!(page.last("notice").unwrap()["text"].as_str().unwrap().contains("still on its way"));
    assert_eq!(t.lobby.roster()["members"][0]["player"], Value::Null);
    let player = t.lobby.game.join().unwrap();
    let claimed = t.lobby.claim(&json!(token)).unwrap();
    t.lobby.seat(claimed, player);
    assert_eq!(t.lobby.roster()["members"][0]["player"], json!(player));
    let seen = snapshot(&mut t.lobby)["players"][player as usize - 1].clone();
    assert_eq!((seen["name"].clone(), seen["color"].clone(), seen["human"].clone()), (json!("Ann"), json!("blue"), json!(true)));
    assert_eq!(t.lobby.claim(&json!(token)), None, "a token already playing cannot be used for a second hotdog");
    // The two sockets of a page close in either order; the member goes when both have.
    t.lobby.leave(member);
    assert_eq!(t.lobby.members.len(), 1, "its hotdog is still connected");
    let (again, again_inbox) = Outbox::memory();
    assert_eq!(t.lobby.enter(again, &json!({"token": token, "name": "Impostor"})), Some(member), "the page comes back with its token");
    assert_eq!(t.lobby.member(member).unwrap().name, "Ann");
    assert!(again_inbox.lock().unwrap()[0].contains("\"welcome\""));
    t.lobby.game.leave(player);
    t.lobby.unseat(member);
    assert_eq!(t.lobby.members.len(), 1);
    assert_eq!(t.lobby.roster()["members"][0]["player"], Value::Null);
    t.lobby.leave(member);
    assert_eq!(t.lobby.members.len(), 0);
    // A client without a page is a guest: named and painted as it asks, checked the same way, with no chat and no voice.
    let watcher = t.page(json!({"name": "Watcher"}));
    let guest = t.lobby.guest(&json!({"name": " Watcher\u{0} ", "color": "yellow"})).unwrap();
    let seat = t.lobby.game.join().unwrap();
    t.lobby.seat(guest, seat);
    assert_eq!((t.lobby.member(guest).unwrap().name.as_str(), t.lobby.member(guest).unwrap().color.as_str()), ("Watcher 2", "yellow"));
    assert_eq!(t.lobby.roster()["members"].as_array().unwrap().iter().find(|m| m["id"] == guest).unwrap()["voice"], false);
    assert!(t.lobby.ready(guest, true));
    assert!(t.lobby.game.player(seat).unwrap().ready);
    assert!(watcher.says("Watcher 2 is ready"));
    t.lobby.game.leave(seat);
    t.lobby.unseat(guest);
    assert!(t.lobby.member(guest).is_none());
    assert!(watcher.says("Watcher 2 left"));
}
#[test]
fn chat_is_capped_at_200_characters_stripped_of_control_characters_rate_limited_and_reaches_every_page() {
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann", "color": "blue"}));
    let b = t.page(json!({"name": "Ben"}));
    let c = t.page(json!({"name": "Cy"}));
    t.handle(&a, json!({"type": "chat", "text": "  hello\u{7}  fridge "}));
    for page in [&a, &b, &c] {
        assert_eq!(page.last("chat").unwrap(), json!({"type": "chat", "from": a.member(), "name": "Ann", "color": "blue", "text": "hello fridge"}));
    }
    // Markup is passed on as the text it is (the page shows it as text); nothing empty is sent at all.
    t.handle(&b, json!({"type": "chat", "text": "<img src=x onerror=alert(1)>"}));
    assert_eq!(a.last("chat").unwrap()["text"], "<img src=x onerror=alert(1)>");
    let count = |page: &Page| page.all("chat").len();
    let had = count(&a);
    for bad in [json!({"type": "chat"}), json!({"type": "chat", "text": ""}), json!({"type": "chat", "text": "\u{0}"}), json!({"type": "chat", "text": ["x"]}), Value::Null, json!("chat"), json!(7), json!({"type": "nonsense"})] {
        t.handle(&b, bad);
    }
    assert_eq!(count(&a), had);
    t.handle(&c, json!({"type": "chat", "text": "y".repeat(900)}));
    assert_eq!(a.last("chat").unwrap()["text"], "y".repeat(200));
    // Five lines every five seconds each.
    t.advance(RATE.1 + 1.);
    let start = count(&a);
    for i in 0..9 {
        t.handle(&a, json!({"type": "chat", "text": format!("line {i}")}));
    }
    assert_eq!(count(&a) - start, RATE.0);
    assert!(a.last("notice").unwrap()["text"].as_str().unwrap().contains("Slow down"));
    assert_eq!(b.all("notice").len(), 0, "only the sender is told");
    t.handle(&b, json!({"type": "chat", "text": "mine still goes"}));
    assert_eq!(c.last("chat").unwrap()["text"], "mine still goes");
    t.advance(RATE.1 + 1.);
    t.handle(&a, json!({"type": "chat", "text": "again"}));
    assert_eq!(c.last("chat").unwrap()["text"], "again");
    // Whoever arrives next is shown the last lines.
    for i in 0..HISTORY + 10 {
        t.advance(RATE.1);
        t.handle(&a, json!({"type": "chat", "text": format!("old {i}")}));
    }
    let d = t.page(json!({"name": "Dee"}));
    let history = d.last("welcome").unwrap()["history"].as_array().unwrap().clone();
    assert_eq!(history.len(), HISTORY);
    assert_eq!(history.last().unwrap()["text"], format!("old {}", HISTORY + 9));
    assert!(a.says("Dee joined"));
}
#[test]
fn voice_signalling_is_relayed_between_two_members_pages_and_to_nobody_else() {
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    let c = t.page(json!({"name": "Cy"}));
    let guest = t.lobby.guest(&json!({"name": "Native"})).unwrap();
    let seat = t.lobby.game.join().unwrap();
    t.lobby.seat(guest, seat);
    let offer = json!({"description": {"type": "offer", "sdp": "v=0\r\n"}});
    t.handle(&a, json!({"type": "signal", "to": b.member(), "data": offer}));
    assert_eq!(b.all("signal"), [json!({"type": "signal", "from": a.member(), "data": offer})]);
    assert!(a.all("signal").is_empty() && c.all("signal").is_empty());
    t.handle(&b, json!({"type": "signal", "to": a.member(), "data": {"candidate": {"candidate": "candidate:1 1 udp 1 192.168.1.2 5000 typ host", "sdpMid": "0", "sdpMLineIndex": 0, "extra": "dropped"}}}));
    assert_eq!(
        a.all("signal"),
        [json!({"type": "signal", "from": b.member(), "data": {"candidate": {"candidate": "candidate:1 1 udp 1 192.168.1.2 5000 typ host", "sdpMid": "0", "sdpMLineIndex": 0, "usernameFragment": null}}})]
    );
    // Not to oneself, not to nobody, not to a guest (it has no page), not from someone who has left, and nothing that is
    // not a description or a candidate.
    let count = |pages: &[&Page]| pages.iter().map(|p| p.all("signal").len()).sum::<usize>();
    let had = count(&[&a, &b, &c]);
    for message in [
        json!({"type": "signal", "to": a.member(), "data": offer}),
        json!({"type": "signal", "to": 99, "data": offer}),
        json!({"type": "signal", "to": guest, "data": offer}),
        json!({"type": "signal", "data": offer}),
        json!({"type": "signal", "to": b.member(), "data": {"description": {"type": "pranswer", "sdp": "x"}}}),
        json!({"type": "signal", "to": b.member(), "data": {"script": "alert(1)"}}),
        json!({"type": "signal", "to": b.member(), "data": "offer"}),
        json!({"type": "signal", "to": b.member()}),
        json!({"type": "signal", "to": b.member(), "data": {"description": {"type": "offer", "sdp": "x".repeat(SIGNAL)}}}),
    ] {
        t.handle(&a, message);
    }
    assert_eq!(count(&[&a, &b, &c]), had);
    t.lobby.leave(c.member());
    t.lobby.game.leave(c.player.unwrap());
    t.lobby.unseat(c.member());
    t.lobby.handle(c.member(), &json!({"type": "signal", "to": a.member(), "data": offer}));
    t.handle(&a, json!({"type": "signal", "to": c.member(), "data": offer}));
    assert_eq!(count(&[&a, &b, &c]), had);
}
#[test]
fn a_speaking_level_reaches_the_other_pages_and_the_speakers_hotdog_and_fades_when_it_is_not_renewed() {
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    let me = a.player.unwrap();
    t.handle(&a, json!({"type": "speaking", "level": 0.8}));
    assert_eq!((b.all("voice").len(), t.lobby.game.talking(me)), (0, 0.), "a microphone that is not live says nothing");
    t.handle(&a, json!({"type": "mic", "state": "live"}));
    t.run(0.1);
    assert_eq!(b.last("roster").unwrap()["members"][0]["mic"], "live");
    t.handle(&a, json!({"type": "speaking", "level": 0.8}));
    assert_eq!(b.last("voice").unwrap(), json!({"type": "voice", "id": a.member(), "level": 0.8}));
    assert_eq!(a.all("voice").len(), 0);
    assert_eq!(t.lobby.game.talking(me), 0.8);
    let seen = snapshot(&mut t.lobby)["players"][me as usize - 1]["talk"].as_f64().unwrap();
    assert!((seen - 0.8).abs() < 1e-6);
    for (sent, taken) in [(json!(7), 1.), (json!(-3), 0.), (json!("loud"), 0.), (json!(0.5), 0.5)] {
        t.handle(&a, json!({"type": "speaking", "level": sent}));
        assert_eq!(t.lobby.game.talking(me), taken);
    }
    t.run(QUIET / 1000. + 0.2);
    assert_eq!(t.lobby.game.talking(me), 0.);
    assert_eq!(b.last("voice").unwrap(), json!({"type": "voice", "id": a.member(), "level": 0.0}));
    t.handle(&a, json!({"type": "speaking", "level": 0.6}));
    t.handle(&a, json!({"type": "mic", "state": "muted"}));
    assert_eq!(t.lobby.game.talking(me), 0., "muting is silence at once");
    t.advance(1.);
    t.handle(&a, json!({"type": "mic", "state": "shouting"}));
    assert_eq!(t.lobby.member(a.member()).unwrap().mic, "muted");
}
#[test]
fn the_demo_has_no_lobby_the_bots_play_at_once_and_rounds_follow_by_themselves() {
    let mut game = Game::new(read_map("assets/maps/counter-siege.json"), &Settings { demo: true, ..Settings::default() });
    assert_eq!(game.phase, Phase::Round);
    let p = game.join().unwrap();
    assert_eq!((game.stage(), game.player(p).unwrap().bot), ("Warmup", true));
    assert!(!game.ready(p, true));
    let late = game.join().unwrap();
    assert!(!game.waiting(late) && game.player(late).unwrap().bot, "a spectator of the demo takes over a seat at once, and a bot goes on playing it");
    let round = game.feast.round;
    for _ in 0..(400. * HZ) as usize {
        game.tick(STEP);
        if game.feast.round > round {
            break;
        }
    }
    assert!(game.feast.round > round, "the next round followed by itself");
    assert_eq!(game.stage(), "Warmup");
}

// ----- Forts: the lobby has teams.

fn fort_map() -> Map {
    read_map("tools/network/fixtures/long-fort.json")
}
#[test]
fn a_forts_lobby_says_its_mode_and_has_teams_a_member_asks_for_one_or_leaves_it_to_the_server_and_the_roster_shows_where_everyone_would_be() {
    let mut t = Table::new(Settings { build: Some(5.), ..Settings::default() }, Some(fort_map()), |_| {});
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben", "team": "ketchup"}));
    let c = t.page(json!({"name": "Cy", "team": "purple"}));
    let welcome = a.last("welcome").unwrap();
    assert_eq!((welcome["mode"].clone(), welcome["teams"].clone()), (json!("forts"), json!(TEAM_NAMES)));
    assert_eq!(lobby::WISHES, ["auto", "mustard", "ketchup"]);
    let teams: Vec<String> = [&a, &b, &c].iter().map(|p| t.lobby.member(p.member()).unwrap().team.clone()).collect();
    assert_eq!(teams, ["auto", "ketchup", "auto"], "an invalid wish is left to the server");
    t.run(0.1);
    let roster = a.last("roster").unwrap();
    assert_eq!((roster["mode"].clone(), roster["teams"].clone(), roster["scores"].clone()), (json!("forts"), json!(TEAM_NAMES), json!([0, 0])));
    // Ben asked for ketchup; the two who did not ask go where there is room, the smaller side first.
    let sides = |roster: &Value| roster["members"].as_array().unwrap().iter().map(|m| (m["name"].as_str().unwrap().to_string(), m["team"].as_str().unwrap().to_string(), m["side"].clone())).collect::<Vec<_>>();
    assert_eq!(sides(&roster), [("Ann".into(), "auto".into(), json!(0)), ("Ben".into(), "ketchup".into(), json!(1)), ("Cy".into(), "auto".into(), json!(0))]);
    assert_eq!(roster["bots"], json!([1, 2]));
    // Asking changes the sides at once, and is announced; anything that is not a team is ignored.
    t.handle(&a, json!({"type": "team", "team": "ketchup"}));
    t.handle(&c, json!({"type": "team", "team": "ketchup"}));
    t.handle(&c, json!({"type": "team", "team": "<b>"}));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert!(sides(&roster).iter().all(|(_, team, side)| team == "ketchup" && *side == json!(1)));
    assert_eq!(roster["bots"], json!([3, 0]));
    assert!(b.says("Ann asks to join the Ketchup Connoisseurs"));
    // A side holds three: a fourth who asks for it is put on the other, and 'auto' hands the choice back.
    let d = t.page(json!({"name": "Dee", "team": "ketchup"}));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!(sides(&roster).iter().map(|s| s.2.clone()).collect::<Vec<_>>(), [json!(1), json!(1), json!(1), json!(0)]);
    assert_eq!(roster["bots"], json!([2, 0]));
    t.handle(&a, json!({"type": "team", "team": "auto"}));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!(sides(&roster).iter().map(|s| (s.1.clone(), s.2.clone())).collect::<Vec<_>>(), [("auto".into(), json!(0)), ("ketchup".into(), json!(1)), ("ketchup".into(), json!(1)), ("ketchup".into(), json!(1))]);
    assert!(b.says("Ann will go where there is room"));
    // The bots' names of both modes are taken.
    let e = t.page(json!({"name": "Tomato"}));
    assert_eq!(t.lobby.member(e.member()).unwrap().name, "Tomato 2");
    // The round starts with those sides: three a side, each hotdog at a start on its side, bots making up the numbers.
    for page in [&a, &b, &c, &d, &e] {
        t.ready(page, true);
    }
    t.run((COUNTDOWN + OPENING + 0.2) as f64);
    assert_eq!(t.lobby.game.stage(), "Build");
    assert_eq!([&a, &b, &c, &d, &e].map(|p| t.hotdog(p).team), [0, 1, 1, 1, 0]);
    let all = &t.lobby.game.feast.players;
    assert_eq!([0, 1].map(|team| all.iter().filter(|p| p.team == team).count()), [3, 3]);
    assert_eq!(all.iter().filter(|p| p.bot).count(), 1);
    assert_eq!(all.iter().find(|p| p.bot).unwrap().team, 0);
    let level = &t.lobby.game.feast.level;
    for p in all {
        assert_eq!(level.side(p.p[0] as f64, p.p[2] as f64), level.fort.as_ref().unwrap().sides[p.team as usize], "{} starts on its side", p.name);
    }
    t.run(0.1);
    let roster = a.last("roster").unwrap();
    assert_eq!(roster["members"].as_array().unwrap().iter().map(|m| m["side"].clone()).collect::<Vec<_>>(), [json!(0), json!(1), json!(1), json!(1), json!(0)]);
    assert_eq!(roster["bots"], json!([1, 0]));
}
#[test]
fn in_a_forts_round_ready_ends_the_build_phase_once_everyone_playing_has_pressed_it_and_the_match_ends_in_the_lobby() {
    let mut t = Table::new(Settings { build: Some(60.), ..Settings::default() }, Some(fort_map()), |_| {});
    let a = t.page(json!({"name": "Ann", "team": "mustard"}));
    let b = t.page(json!({"name": "Ben", "team": "ketchup"}));
    t.ready(&a, true);
    t.ready(&b, true);
    t.run((COUNTDOWN + OPENING + 0.2) as f64);
    assert_eq!(t.lobby.game.stage(), "Build");
    assert_eq!((t.hotdog(&a).ready, t.hotdog(&b).ready), (false, false));
    assert!(t.ready(&a, true));
    t.run(1.);
    assert_eq!(t.lobby.game.stage(), "Build");
    assert!(t.lobby.game.feast.alert.contains("1 OF 2 READY"), "{}", t.lobby.game.feast.alert);
    assert!(b.says("Ann is ready"));
    t.handle(&b, json!({"type": "ready", "ready": true}));
    t.run(0.1);
    assert_eq!(t.lobby.game.stage(), "Fight");
    assert!(a.says("The forcefield is down"));
    // Three rounds to the Mustard Eaters, each by carrying the bacon home; then everyone is back in the fridge, not
    // ready, and may change sides.
    for round in 0..3 {
        if t.lobby.game.stage() == "Build" {
            t.ready(&a, true);
            t.ready(&b, true);
            t.run(0.1);
        }
        assert_eq!(t.lobby.game.stage(), "Fight");
        let me = a.player.unwrap() as usize - 1;
        t.lobby.game.feast.players[me].p = [0., FLOOR + 0.12, 3.1];
        t.run(0.1);
        t.lobby.game.feast.players[me].p = [-2.75, FLOOR, 3.1];
        t.run(0.1);
        assert_eq!(t.lobby.game.feast.stage, Stage::RoundOver);
        assert_eq!(a.last("roster").unwrap()["scores"][0], json!(round + 1));
        t.run((INTERMISSION + 0.2) as f64);
    }
    assert_eq!(t.lobby.game.stage(), "Lobby");
    assert!(a.says("Match over: back in the fridge"));
    let roster = a.last("roster").unwrap();
    assert_eq!(roster["scores"], json!([0, 0]));
    assert_eq!(roster["members"].as_array().unwrap().iter().map(|m| (m["ready"].clone(), m["side"].clone())).collect::<Vec<_>>(), [(json!(false), json!(0)), (json!(false), json!(1))]);
    assert_eq!(t.lobby.game.feast.fort.as_ref().unwrap().match_no, 2, "the next match is another");
    t.handle(&a, json!({"type": "team", "team": "ketchup"}));
    t.run(0.1);
    assert_eq!(a.last("roster").unwrap()["members"].as_array().unwrap().iter().map(|m| m["side"].clone()).collect::<Vec<_>>(), [json!(1), json!(1)]);
}
#[test]
fn a_siege_lobby_has_no_teams_the_roster_says_siege_and_a_team_message_does_nothing() {
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann", "team": "mustard"}));
    t.run(0.1);
    let roster = a.last("roster").unwrap();
    assert_eq!(roster["mode"], "siege");
    assert!(roster.get("teams").is_none() && roster["members"][0].get("team").is_none() && roster["members"][0].get("side").is_none());
    assert_eq!(a.last("welcome").unwrap()["mode"], "siege");
    assert!(!t.lobby.team(a.member(), &json!("ketchup")));
    t.handle(&a, json!({"type": "team", "team": "ketchup"}));
    assert!(!a.all("system").iter().any(|m| m["text"].as_str().unwrap().contains("asks to join")));
}

// ----- The leader and the choice: one server, every map.

#[test]
fn the_first_player_seated_with_a_page_leads_the_lobby_keeps_the_lead_and_hands_it_on_in_the_order_they_arrived() {
    let mut t = Table::plain();
    let (outbox, early) = Outbox::memory();
    let slow = t.lobby.enter(outbox, &json!({"name": "Slow"})).unwrap();
    let early = Page { inbox: early, member: Some(slow), player: None };
    t.run(0.1);
    // Nobody is seated yet: nobody leads. The page that is still loading its game does not.
    assert_eq!(early.last("roster").unwrap()["leader"], Value::Null);
    assert_eq!(snapshot(&mut t.lobby)["leader"], 0);
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    t.run(0.1);
    assert_eq!(b.last("roster").unwrap()["leader"], json!(a.member()));
    assert!(b.says("Ann leads the lobby and chooses the game"));
    assert_eq!(snapshot(&mut t.lobby)["leader"], json!(a.player.unwrap()), "the game sockets are told which hotdog leads");
    // Slow's game arrives: Ann keeps the lead.
    let token = json!(t.lobby.member(slow).unwrap().token.clone());
    let claimed = t.lobby.claim(&token).unwrap();
    let seat = t.lobby.game.join().unwrap();
    t.lobby.seat(claimed, seat);
    t.run(0.1);
    assert_eq!(b.last("roster").unwrap()["leader"], json!(a.member()));
    // Ann goes: the lead passes to the first who arrived of those left (Slow, before Ben), and then to Ben.
    t.lobby.game.leave(a.player.unwrap());
    t.lobby.unseat(a.member());
    t.lobby.leave(a.member());
    t.run(0.1);
    assert_eq!(b.last("roster").unwrap()["leader"], json!(slow));
    assert!(b.all("system").iter().any(|m| m["text"].as_str().unwrap().starts_with("Slow leads the lobby")));
    t.lobby.game.leave(seat);
    t.lobby.unseat(slow);
    t.lobby.leave(slow);
    t.run(0.1);
    assert_eq!(b.last("roster").unwrap()["leader"], json!(b.member()));
    // A client without a page (the native one) has nowhere to choose from, so it never leads.
    let mut solo = Table::plain();
    let guest = solo.lobby.guest(&json!({"name": "Native"})).unwrap();
    let seat = solo.lobby.game.join().unwrap();
    solo.lobby.seat(guest, seat);
    solo.run(0.1);
    assert_eq!(solo.lobby.lead(), None);
    assert!(!solo.lobby.choose(guest, Some(&json!("forts")), None));
    assert_eq!(solo.lobby.choice.name, "counter-siege");
}
/// A folder of maps for a test: some of assets/maps, and anything else written into it.
struct Folder(PathBuf);
impl Folder {
    fn new(names: &[&str]) -> Folder {
        let path = std::env::temp_dir().join(format!("hotdog-maps-{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir_all(&path).unwrap();
        for name in names {
            std::fs::copy(maps_folder().join(format!("{name}.json")), path.join(format!("{name}.json"))).unwrap();
        }
        Folder(path)
    }
    fn scanner(&self) -> Box<dyn Fn() -> Catalogue + Send> {
        let path = self.0.clone();
        Box::new(move || scan(&path))
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn notice(page: &Page) -> String {
    page.last("notice").map_or(String::new(), |m| m["text"].as_str().unwrap().to_string())
}
#[test]
fn only_the_leader_chooses_only_in_the_lobby_before_the_countdown_and_only_a_map_that_is_in_assets_maps_and_can_be_played() {
    let folder = Folder::new(&["counter-siege", "horde-night", "pillow-fort"]);
    std::fs::write(folder.0.join("broken.json"), r#"{"format":"hotdog-map","version":1,"bounds":[0,0,2,1],"starts":[{"p":[0.2,0.2]}],"spawns":[]}"#).unwrap();
    std::fs::write(folder.0.join("random.json"), "{}").unwrap();
    let scanner = folder.scanner();
    let mut t = Table::new(Settings { countdown: 0.3, opening: 0.3, ..Settings::default() }, None, |o| o.maps = scanner);
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    t.run(0.1);
    let names: Vec<Value> = a.last("roster").unwrap()["maps"].as_array().unwrap().iter().map(|e| e["name"].clone()).collect();
    assert_eq!(names, [json!("counter-siege"), json!("horde-night"), json!("pillow-fort")], "siege maps first, then Forts; neither the broken map nor the reserved name");
    assert_eq!(a.last("roster").unwrap()["choice"], json!({"name": "counter-siege", "title": "Counter Siege", "mode": "siege", "theme": "kitchen"}));
    // Somebody else is told who chooses.
    assert!(!t.lobby.choose(b.member(), Some(&json!("forts")), None));
    assert_eq!(notice(&b), "Only the leader, Ann, can choose the game.");
    t.handle(&b, json!({"type": "choose", "map": "horde-night"}));
    assert_eq!(t.lobby.choice.name, "counter-siege");
    // Names are bare words of the folder: no paths, no extensions, nothing else; a file that is not a valid map is refused.
    for bad in [json!("../counter-siege"), json!(".."), json!("counter-siege.json"), json!("/etc/passwd"), json!("assets/maps/horde-night"), json!(" horde-night"), json!(""), json!(42), Value::Null, json!({}), json!(["horde-night"])] {
        assert!(!t.lobby.choose(a.member(), None, Some(&bad)), "{bad}");
        assert_eq!(notice(&a), "That is not the name of a map.");
    }
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("midnight-snack"))));
    assert_eq!(notice(&a), "There is no map called midnight-snack.");
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("broken"))));
    assert!(notice(&a).starts_with("The map broken cannot be played: ") && notice(&a).contains("spawn"), "{}", notice(&a));
    for bad in [json!("FORTS"), json!("ctf"), json!(7)] {
        assert!(!t.lobby.choose(a.member(), Some(&bad), None));
    }
    assert_eq!(notice(&a), "There is no such mode.");
    assert!(!t.lobby.choose(a.member(), Some(&json!("forts")), Some(&json!("horde-night"))));
    assert_eq!(notice(&a), "Horde Night has no Forts layout (two bases, a field and a bacon spot): it cannot be played as Forts.");
    assert!(!t.lobby.choose(a.member(), None, None));
    assert_eq!(t.lobby.choice.name, "counter-siege");
    // The leader's own page: a map of the folder is taken; the same again changes nothing.
    t.handle(&a, json!({"type": "choose", "map": "horde-night"}));
    assert_eq!(t.lobby.choice.name, "horde-night");
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("horde-night"))));
    // Not during the countdown, the start sequence or the round.
    t.ready(&a, true);
    t.ready(&b, true);
    t.run(0.1);
    assert!(t.lobby.game.countdown.is_some_and(|c| c > 0.));
    assert!(!t.lobby.choose(a.member(), Some(&json!("forts")), None));
    assert_eq!(notice(&a), "The game can only be changed in the lobby, before the countdown.");
    t.run(0.3);
    assert_eq!(t.lobby.game.stage(), "Opening");
    assert!(!t.lobby.choose(a.member(), Some(&json!("forts")), None));
    t.run(0.4);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    assert!(!t.lobby.choose(a.member(), Some(&json!("forts")), None));
    assert_eq!(t.lobby.choice.name, "horde-night");
    // Back in the lobby the leader can choose again.
    let winner = t.lobby.player(a.member());
    t.finish(winner);
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), None));
    assert_eq!(t.lobby.choice.name, "pillow-fort");
}
#[test]
fn a_change_of_mode_makes_everyone_unready_and_forts_brings_the_team_choice_and_the_sides_which_a_siege_has_not() {
    let mut t = Table::plain();
    let a = t.page(json!({"name": "Ann", "team": "ketchup"}));
    let b = t.page(json!({"name": "Ben"}));
    let c = t.page(json!({"name": "Cy"}));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!(roster["mode"], "siege");
    assert!(roster.get("teams").is_none() && roster["members"][0].get("side").is_none());
    assert!(!t.lobby.team(b.member(), &json!("mustard")));
    for page in [&b, &c] {
        t.ready(page, true);
    }
    t.run(0.1);
    let ready = |page: &Page| page.last("roster").unwrap()["members"].as_array().unwrap().iter().map(|m| m["ready"].clone()).collect::<Vec<_>>();
    assert_eq!(ready(&b), [json!(false), json!(true), json!(true)]);
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), None));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!(ready(&b), [json!(false), json!(false), json!(false)], "everyone is unready");
    assert!(b.says("Ann chose Forts on Countertop Fort (kitchen): ready up again"));
    assert_eq!(roster["choice"], json!({"name": "pillow-fort", "title": "Countertop Fort", "mode": "forts", "theme": "kitchen"}));
    assert_eq!(roster["playing"], json!({"name": "counter-siege", "title": "Counter Siege", "mode": "siege", "theme": "kitchen"}));
    // The lobby has teams although the siege is still what the game plays: Ann asked for ketchup when she arrived; the
    // others go where there is room.
    assert!(!t.lobby.game.forts());
    assert_eq!((roster["mode"].clone(), roster["teams"].clone(), roster["scores"].clone()), (json!("forts"), json!(TEAM_NAMES), json!([0, 0])));
    let sides = |roster: &Value| roster["members"].as_array().unwrap().iter().map(|m| m["side"].clone()).collect::<Vec<_>>();
    assert_eq!(sides(&roster), [json!(1), json!(0), json!(0)]);
    assert_eq!(roster["bots"], json!([1, 2]));
    assert!(t.lobby.team(c.member(), &json!("ketchup")));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!(sides(&roster), [json!(1), json!(0), json!(1)]);
    assert_eq!(roster["bots"], json!([2, 1]));
    let snap = snapshot(&mut t.lobby);
    let expected: Value = [(a.player.unwrap(), 1), (b.player.unwrap(), 0), (c.player.unwrap(), 1)].iter().map(|(p, s)| (p.to_string(), json!(s))).collect::<serde_json::Map<_, _>>().into();
    assert_eq!(snap["sides"], expected, "the fridge is told the sides by hotdog");
    assert_eq!(snap["choice"]["mode"], "forts");
    // A map of the same mode makes everyone unready too.
    t.ready(&b, true);
    assert!(t.lobby.choose(a.member(), None, Some(&json!("jungle-fort"))));
    assert!(!t.hotdog(&b).ready);
    // Back to a siege: the mode is the game's, so on the map chosen (Jungle Fort holds a siege as well as Forts), and the
    // teams are gone again (what was asked for is kept for next time).
    assert!(t.lobby.choose(a.member(), Some(&json!("siege")), None));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!((roster["choice"]["name"].clone(), roster["choice"]["mode"].clone()), (json!("jungle-fort"), json!("siege")));
    assert!(b.says("Ann chose Siege on Jungle Fort (jungle): ready up again"));
    assert!(roster.get("teams").is_none() && roster["members"][0].get("side").is_none());
    assert!(snapshot(&mut t.lobby).get("sides").is_none());
    assert_eq!(t.lobby.member(c.member()).unwrap().team, "ketchup");
    // A siege map chosen now is a siege: the mode chosen stays.
    assert!(t.lobby.choose(a.member(), None, Some(&json!("counter-siege"))));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("counter-siege", "siege"));
    // Forts again: Counter Siege cannot hold it, so on the Forts map last chosen.
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), None));
    assert_eq!(t.lobby.choice.name, "jungle-fort");
}
/// The mode is the game's (kitchen_sim::rules `Mode`): the leader chooses it and the map apart; the map's own mode is
/// only what a map chosen alone plays before a mode has been chosen.
#[test]
fn the_mode_and_the_map_are_chosen_apart_a_map_alone_keeps_a_chosen_gun_mode_and_one_that_cannot_hold_it_is_refused_with_the_reason() {
    let mut t = Table::timed(0.2, 0.3);
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    // The leader's list says what each map can be played in, and why not in the rest.
    let roster = a.last("roster").unwrap();
    let listed = |name: &str| roster["maps"].as_array().unwrap().iter().find(|e| e["name"] == name).cloned().unwrap();
    assert_eq!(listed("pillow-fort")["modes"], json!(["siege", "forts", "ffa"]));
    assert_eq!(listed("counter-siege")["modes"], json!(["siege", "ffa", "cards"]));
    assert_eq!(listed("horde-night")["modes"], json!(["siege", "ffa"]));
    assert_eq!(listed("kung-fu-dojo")["modes"], json!(["kungfu"]));
    assert_eq!(listed("horde-night")["why"]["forts"], json!("Horde Night has no Forts layout (two bases, a field and a bacon spot): it cannot be played as Forts."));
    assert_eq!(listed("kung-fu-dojo")["why"]["ffa"], json!("Kung Fu Dojo is a Kung Fu map: it cannot be played as Free For All."));
    assert_eq!((listed("pillow-fort")["mode"].clone(), listed("pillow-fort")["why"]["cards"].clone()), (json!("forts"), json!("Fully Loaded is played on Counter Siege.")));
    // Before a mode is chosen, a map alone plays the mode it was made for, as a link to it does.
    assert!(!t.lobby.mode_set);
    assert!(t.lobby.choose(a.member(), None, Some(&json!("pillow-fort"))));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("pillow-fort", "forts"));
    assert!(t.lobby.choose(a.member(), None, Some(&json!("counter-siege"))));
    assert_eq!(t.lobby.choice.mode, "siege");
    // A siege chosen, then a Forts map: a siege on it.
    assert!(!t.lobby.choose(a.member(), Some(&json!("siege")), None), "a siege on Counter Siege already");
    assert!(t.lobby.mode_set);
    assert!(t.lobby.choose(a.member(), None, Some(&json!("pillow-fort"))));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("pillow-fort", "siege"));
    assert!(a.says("Ann chose Siege on Countertop Fort (kitchen): ready up again"));
    // It is played so: five hotdogs, no teams, no forcefield; the frame's rules say it, and so does the roster.
    t.ready(&a, true);
    t.run(0.8);
    let game = &t.lobby.game;
    assert_eq!((game.map.name.as_str(), game.seats(), game.forts(), game.stage(), game.rules().text().as_str()), ("Countertop Fort", 5, false, "Warmup", "mode=siege"));
    assert_eq!(snapshot(&mut t.lobby)["rules"], json!("mode=siege"));
    assert_eq!(a.last("roster").unwrap()["playing"], json!({"name": "pillow-fort", "title": "Countertop Fort", "mode": "siege", "theme": "kitchen"}));
    assert_eq!(lobby::played(&t.lobby.game), "Countertop Fort (siege)");
    let winner = t.lobby.player(a.member());
    t.finish(winner);
    // Free For All chosen keeps the map; a map that cannot hold it is refused with the reason, and the choice stays.
    assert!(t.lobby.choose(a.member(), Some(&json!("ffa")), None));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("pillow-fort", "ffa"));
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("kung-fu-dojo"))));
    assert_eq!(notice(&a), "Kung Fu Dojo is a Kung Fu map: it cannot be played as Free For All.");
    assert!(t.lobby.choose(a.member(), None, Some(&json!("horde-night"))));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("horde-night", "ffa"));
    // Forts, which Horde Night cannot hold: to the Forts map last chosen; a siege map chosen then is refused.
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), None));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("pillow-fort", "forts"));
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("treetop-decks"))));
    assert_eq!(notice(&a), "Treetop Decks has no Forts layout (two bases, a field and a bacon spot): it cannot be played as Forts.");
    assert_eq!(t.lobby.choice.name, "pillow-fort");
    // Kung Fu and Fully Loaded keep to their own maps, and a map chosen alone while either is plays its own mode.
    assert!(t.lobby.choose(a.member(), Some(&json!("kungfu")), None));
    assert_eq!(t.lobby.choice.name, "kung-fu-dojo");
    assert!(t.lobby.choose(a.member(), None, Some(&json!("moonlit-fort"))));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("moonlit-fort", "forts"));
    // Siege on Random draws from every map but the dojo.
    assert!(t.lobby.choose(a.member(), Some(&json!("siege")), Some(&json!("random"))));
    assert_eq!((t.lobby.choice.random, t.lobby.choice.mode.as_str()), (true, "siege"));
    // A lobby the server starts with --mode keeps it for a map chosen alone.
    let mut started = Table::new(Settings::default(), Some(read_map("assets/maps/pillow-fort.json")), |o| o.chosen = true);
    started.lobby.game = Game::with_rules(read_map("assets/maps/pillow-fort.json"), &Settings::default(), Rules::parse("mode=ffa"));
    let x = started.page(json!({"name": "X"}));
    started.run(0.1);
    assert!(started.lobby.mode_set);
    started.lobby.choice = started.lobby.choice.clone().played_as("ffa");
    assert!(started.lobby.choose(x.member(), None, Some(&json!("counter-siege"))));
    assert_eq!(started.lobby.choice.mode, "ffa");
}
#[test]
fn the_next_round_plays_the_chosen_map_and_mode_on_a_fresh_game_with_everyone_in_it_as_they_were_siege_forts_siege_on_one_server() {
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.6, build: Some(30.), ..Settings::default() }, None, |_| {});
    let a = t.page(json!({"name": "Ann", "color": "blue"}));
    let b = t.page(json!({"name": "Ben", "color": "green", "team": "ketchup"}));
    let c = t.page(json!({"name": "Cy", "color": "black"}));
    t.run(0.1);
    let pages = [&a, &b, &c];
    let ready = |t: &mut Table| {
        for page in pages {
            t.ready(page, true);
        }
    };
    let mut switches = vec![];
    let run = |t: &mut Table, seconds: f64, switches: &mut Vec<std::collections::BTreeMap<u32, u32>>| {
        for _ in 0..(seconds * HZ).round() as usize {
            t.advance(1000. / HZ);
            t.lobby.game.tick(STEP);
            if let Some(seats) = t.lobby.tick() {
                switches.push(seats);
            }
        }
    };
    // A siege round on the map it started with: no new game.
    ready(&mut t);
    run(&mut t, 0.4, &mut switches);
    assert_eq!(t.lobby.game.stage(), "Opening");
    assert!(switches.is_empty());
    run(&mut t, 0.6, &mut switches);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    let winner = t.lobby.player(a.member());
    t.finish(winner);
    // Forts on Moonlit Fort: when the fridge opens a Forts game takes over, everyone seated in it under their own names and colours.
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), Some(&json!("moonlit-fort"))));
    ready(&mut t);
    let round = t.lobby.game.feast.round;
    run(&mut t, 0.25, &mut switches);
    assert_eq!(t.lobby.game.stage(), "Opening");
    assert_eq!(switches.len(), 1);
    let game = &t.lobby.game;
    assert_eq!((game.map.name.as_str(), game.map.mode.as_str(), game.seats(), game.feast.fort.as_ref().unwrap().build), ("Moonlit Fort", "forts", 6, 30.));
    assert_eq!(game.feast.round, round, "the round numbers go on");
    let seats: Vec<u32> = pages.iter().map(|p| t.lobby.player(p.member()).unwrap()).collect();
    assert_eq!(pages.iter().map(|p| switches[0][&p.member()]).collect::<Vec<_>>(), seats, "each seat is told its new hotdog");
    let snap = snapshot(&mut t.lobby);
    let seen: Vec<Value> = seats.iter().map(|id| {
        let p = &snap["players"][*id as usize - 1];
        json!([p["id"], p["name"], p["color"], p["human"], p["bot"]])
    }).collect();
    assert_eq!(seen, [json!([1, "Ann", "blue", true, false]), json!([2, "Ben", "green", true, false]), json!([3, "Cy", "black", true, false])]);
    assert_eq!(a.last("roster").unwrap()["playing"], json!({"name": "moonlit-fort", "title": "Moonlit Fort", "mode": "forts", "theme": "night"}));
    assert!(t.lobby.game.opening < 0.1, "the start sequence goes on from where it was");
    run(&mut t, 0.6, &mut switches);
    assert_eq!(t.lobby.game.stage(), "Build");
    assert!(t.lobby.game.feast.round > round);
    assert_eq!(pages.map(|p| t.hotdog(p).team), [0, 1, 0], "Ben asked for ketchup; the others went where there was room");
    assert_eq!([0, 1].map(|team| t.lobby.game.feast.players.iter().filter(|p| p.team == team).count()), [3, 3]);
    // A page arriving during the match waits for the next round in the new game.
    let d = t.page(json!({"name": "Dee"}));
    assert!(t.lobby.game.waiting(d.player.unwrap()));
    t.finish(None);
    assert!(a.says("Match over: back in the fridge"));
    assert!(!t.lobby.game.waiting(t.lobby.player(d.member()).unwrap()));
    // Back to a siege, on Horde Night: five hotdogs, no teams, and Dee plays this one.
    assert!(t.lobby.choose(a.member(), Some(&json!("siege")), Some(&json!("horde-night"))));
    for page in [&a, &b, &c, &d] {
        t.ready(page, true);
    }
    run(&mut t, 0.25, &mut switches);
    assert_eq!(switches.len(), 2);
    assert_eq!((t.lobby.game.map.name.as_str(), t.lobby.game.seats(), t.lobby.game.forts()), ("Horde Night", 5, false));
    run(&mut t, 0.6, &mut switches);
    assert_eq!(t.lobby.game.stage(), "Warmup");
    let roll: Vec<(String, bool)> = t.lobby.game.feast.players.iter().map(|p| (p.name.clone(), p.bot)).collect();
    assert_eq!(roll, [("Ann".into(), false), ("Ben".into(), false), ("Cy".into(), false), ("Dee".into(), false), ("SESAME".into(), true)]);
    assert!(a.last("roster").unwrap().get("teams").is_none());
    // Whoever leaves the new game leaves it properly: a bot takes the place.
    let cy = t.lobby.player(c.member()).unwrap();
    t.lobby.game.leave(cy);
    t.lobby.unseat(c.member());
    t.lobby.leave(c.member());
    assert_eq!(t.lobby.game.feast.players.iter().filter(|p| p.bot).count(), 2);
}
#[test]
fn random_draws_a_map_of_the_chosen_mode_each_time_the_fridge_opens_never_the_one_just_played_while_the_mode_has_another() {
    let dice = Arc::new(AtomicU64::new(0f64.to_bits()));
    let roll = dice.clone();
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |o| o.draw = Box::new(move || f64::from_bits(roll.load(Ordering::Relaxed))));
    let a = t.page(json!({"name": "Ann"}));
    let b = t.page(json!({"name": "Ben"}));
    t.run(0.1);
    // (Random draws from the maps that can hold the mode: a siege from every map but the dojo, Forts from the Forts maps.)
    let siege: Vec<String> = t.lobby.maps.iter().filter(|e| lobby::refusal(e, "siege").is_none()).map(|e| e.name.clone()).collect();
    let forts: Vec<String> = t.lobby.maps.iter().filter(|e| lobby::refusal(e, "forts").is_none()).map(|e| e.name.clone()).collect();
    assert!(siege.len() == 13 && forts.len() == 4 && siege.iter().any(|name| name == "pillow-fort"), "{siege:?} {forts:?}");
    let go = |t: &mut Table| {
        for page in [&a, &b] {
            t.ready(page, true);
        }
        t.run(0.25);
        assert_eq!(t.lobby.game.stage(), "Opening");
    };
    assert!(t.lobby.choose(a.member(), None, Some(&json!("random"))));
    t.run(0.1);
    let roster = b.last("roster").unwrap();
    assert_eq!(roster["choice"], json!({"name": "random", "title": "Random", "mode": "siege", "theme": "", "random": true}));
    assert!(b.says("Ann chose Siege on a map drawn at random: ready up again"));
    assert_eq!(snapshot(&mut t.lobby)["choice"], roster["choice"]);
    // The fridge opens: the first siege map (the dice say 0); the lobby still says Random, and what is played is the map drawn.
    go(&mut t);
    assert_eq!(t.lobby.current.name, siege[0]);
    let roster = b.last("roster").unwrap();
    assert_eq!((roster["choice"]["name"].clone(), roster["playing"]["name"].clone()), (json!("random"), json!(siege[0])));
    let winner = t.lobby.player(a.member());
    t.finish(winner);
    // Random stays chosen; the same dice give another map, since the one just played is left out.
    go(&mut t);
    assert_eq!(t.lobby.current.name, siege[1]);
    let winner = t.lobby.player(a.member());
    t.finish(winner);
    dice.store(0.9999f64.to_bits(), Ordering::Relaxed);
    go(&mut t);
    assert_eq!(&t.lobby.current.name, siege.last().unwrap());
    let winner = t.lobby.player(a.member());
    t.finish(winner);
    // Random for Forts: the mode button keeps Random; a Forts map is drawn.
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), Some(&json!("random"))));
    assert_eq!((t.lobby.choice.name.as_str(), t.lobby.choice.mode.as_str()), ("random", "forts"));
    dice.store(0.5f64.to_bits(), Ordering::Relaxed);
    go(&mut t);
    assert!(t.lobby.game.forts());
    // (Not the one just played: the last siege map drawn was the last of the Forts maps, played as a siege.)
    let fresh: Vec<&String> = forts.iter().filter(|name| *name != siege.last().unwrap()).collect();
    assert_eq!(&t.lobby.current.name, fresh[(0.5 * fresh.len() as f64).floor() as usize]);
    t.finish(None);
    assert!(t.lobby.choose(a.member(), Some(&json!("siege")), None));
    assert_eq!(t.lobby.choice.name, "random", "each mode remembers what was chosen for it");
    // A mode with a single map draws it every time.
    let folder = Folder::new(&["counter-siege", "pillow-fort"]);
    let scanner = folder.scanner();
    let mut one = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |o| {
        o.maps = scanner;
        o.draw = Box::new(|| 0.7);
    });
    let x = one.page(json!({"name": "X"}));
    one.run(0.1);
    assert!(one.lobby.choose(x.member(), Some(&json!("forts")), Some(&json!("random"))));
    for _ in 0..2 {
        one.ready(&x, true);
        one.run(0.25);
        assert_eq!(one.lobby.current.name, "pillow-fort");
        one.finish(None);
    }
    // The lobby can start on Random (the server's --map random).
    let start = Table::new(Settings::default(), Some(read_map("assets/maps/pillow-fort.json")), |o| o.random = true);
    assert_eq!((start.lobby.choice.name.as_str(), start.lobby.choice.mode.as_str(), start.lobby.current.name.as_str()), ("random", "forts", "pillow-fort"));
}
#[test]
fn a_map_that_is_not_in_the_folder_is_listed_after_it_under_its_files_name_and_can_be_chosen_again() {
    let mut map = read_map("tools/network/fixtures/long-fort.json");
    map.name = "Long Fort!".into();
    let t = Table::new(Settings::default(), Some(map), |o| o.choice = Some("My Fort".into()));
    assert_eq!(t.lobby.current.name, "my-fort");
    assert_eq!(t.lobby.maps.last().unwrap().name, "my-fort");
    let unnamed = Table::new(Settings::default(), Some(read_map("tools/network/fixtures/awkward.json")), |_| {});
    assert_eq!(unnamed.lobby.current.name, "awkward-fixture");
}

// ----- The game's server-side parts: seats, commands, frames.

/// A game whose round is under way with one person seated (hotdog 1).
fn round_on(map: Map, settings: Settings) -> Game {
    let mut game = Game::new(map, &Settings { countdown: 0., opening: 0., ..settings });
    let p = game.join().unwrap();
    assert!(game.ready(p, true));
    game.tick(STEP);
    game.tick(STEP);
    assert_eq!(game.phase, Phase::Round);
    game
}
fn fresh() -> Game {
    round_on(read_map("assets/maps/counter-siege.json"), Settings::default())
}
/// Nobody acts for the hotdogs (they stand at the back of the counter) and the wave never comes; the bowl has come to
/// rest where the map puts it.
fn still() -> Game {
    let mut s = fresh();
    for (i, q) in s.feast.players.iter_mut().enumerate() {
        (q.bot, q.p) = (false, [-1.6 + (i as f32 + 1.) * 0.1, FLOOR, 3.55]);
    }
    s.feast.warmup = f32::INFINITY;
    for _ in 0..(1.5 * HZ) as usize {
        s.tick(STEP);
    }
    s
}
/// `n` commands numbered from `first`, the stick pushed `stick` while facing `yaw`, `held` held throughout and `edge`
/// pressed in the first.
fn walking(first: u32, n: usize, stick: Vec2, yaw: f32, held: u16, edge: u16) -> Vec<Command> {
    (0..n).map(|k| Command::new(first + k as u32, stick, yaw, 0., held | if k == 0 { edge } else { 0 }, 0, None)).collect()
}
/// The yaw that faces the way from `a` to `b` ([x, z]).
fn facing(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).atan2(a[1] - b[1])
}
/// Each command arrives in time for its step, as a steady client's do: one, then a step.
fn play(game: &mut Game, id: u32, commands: &[Command]) {
    for c in commands {
        game.commands(id, std::slice::from_ref(c));
        game.tick(STEP);
    }
}
fn at(game: &Game, id: u32) -> [f32; 3] {
    game.player(id).unwrap().p
}
fn frame_json(game: &Game) -> Value {
    serde_json::to_value(game.frame()).unwrap()
}
#[test]
fn one_human_and_four_active_bots_fill_the_five_hotdog_roster() {
    let s = fresh();
    assert_eq!(s.players(), 5);
    assert_eq!(s.feast.players.iter().filter(|p| p.bot).count(), 4);
    // The loadout of every gun mode, a person's as a bot's: the pistol's magazine of twelve and 48 in reserve.
    assert_eq!((s.player(1).unwrap().ammo[1], s.player(1).unwrap().reserve[1]), (12, 48));
    assert_eq!((s.player(2).unwrap().ammo[1], s.player(2).unwrap().reserve[1]), (12, 48));
    // In the lobby, somebody new takes over a bot's seat, with the same loadout.
    let mut lobby = Game::new(read_map("assets/maps/counter-siege.json"), &Settings::default());
    assert_eq!(lobby.players(), 0, "nobody is listed until somebody joins");
    let first = lobby.join().unwrap();
    let second = lobby.join().unwrap();
    assert_eq!((first, second, lobby.players()), (1, 2, 5));
    assert!(!lobby.player(second).unwrap().bot);
    assert_eq!((lobby.player(second).unwrap().ammo[1], lobby.player(second).unwrap().reserve[6]), (12, 180));
    for _ in 0..3 {
        lobby.join().unwrap();
    }
    assert_eq!(lobby.join(), None);
}
#[test]
fn a_command_walks_only_as_far_as_its_pace_whatever_it_says_and_a_pickup_out_of_reach_is_refused() {
    let mut s = still();
    s.feast.players[0].p = [0., FLOOR, 2.4];
    s.tick(STEP);
    let before = Vec3::from_array(at(&s, 1));
    // Half a second walking forward, at the walking pace: 0.285 m. A command has no place in it to claim.
    play(&mut s, 1, &walking(1, 30, Vec2::new(0., -1.), -std::f32::consts::FRAC_PI_2, 0, 0));
    let walked = Vec3::from_array(at(&s, 1)).distance(before);
    assert!((walked - WALK as f32 * 0.5).abs() < 1e-3, "walked {walked}");
    // A stick pushed past its rim (a modified client's) walks no faster: each command is a step's worth at most.
    let mut bytes = kitchen_sim::net::Batch { ack: 0, resync: false, time: 0, commands: walking(31, 1, Vec2::ZERO, 0., 0, 0) }.encode();
    let stick = bytes.len() - 8;
    (bytes[stick], bytes[stick + 1]) = (127, 0x81);
    let pushed = kitchen_sim::net::Batch::decode(&bytes).unwrap().commands;
    let here = Vec3::from_array(at(&s, 1));
    play(&mut s, 1, &pushed);
    assert!(Vec3::from_array(at(&s, 1)).distance(here) <= (WALK / HZ) as f32 + 1e-5);
    // Collecting a pickup across the counter is refused.
    let mut far = walking(32, 1, Vec2::ZERO, 0., 0, 0);
    far[0].pickup = Some(3);
    let owned = s.player(1).unwrap().owned;
    play(&mut s, 1, &far);
    assert_eq!(s.player(1).unwrap().owned, owned);
    assert_eq!(s.applied(1), 32);
}
/// Two people standing on one pad both reach for its pickup in the same step: one of them has it, the first seat's,
/// whichever of their commands reached the server first (each step applies the seats' commands in seat order).
#[test]
fn a_pickup_two_people_reach_for_in_the_same_step_goes_to_one_of_them_the_first_seat_whoever_sent_first() {
    for first in [1, 2] {
        let mut game = Game::new(read_map("assets/maps/counter-siege.json"), &Settings { countdown: 0., opening: 0., ..Settings::default() });
        let (a, b) = (game.join().unwrap(), game.join().unwrap());
        assert!(game.ready(a, true) && game.ready(b, true));
        game.tick(STEP);
        game.tick(STEP);
        assert_eq!(game.phase, Phase::Round);
        game.feast.warmup = f32::INFINITY;
        for (i, q) in game.feast.players.iter_mut().enumerate() {
            (q.bot, q.p) = (false, [-1.6 + (i as f32 + 1.) * 0.1, FLOOR, 3.55]);
        }
        let pad = game.feast.pickups[0].p;
        assert_eq!(game.feast.pickups[0].weapon, 2);
        game.feast.players[0].p = [pad[0] - 0.05, FLOOR, pad[2]];
        game.feast.players[1].p = [pad[0] + 0.05, FLOOR, pad[2]];
        game.tick(STEP);
        let reach = || {
            let mut c = walking(1, 1, Vec2::ZERO, 0., 0, 0);
            c[0].pickup = Some(0);
            c
        };
        for id in if first == 1 { [a, b] } else { [b, a] } {
            game.commands(id, &reach());
        }
        game.tick(STEP);
        let (p, q) = (game.player(a).unwrap(), game.player(b).unwrap());
        assert!(p.owned[2] && !q.owned[2], "sent first: {first}");
        assert_eq!((p.ammo[2], q.ammo[2]), (4, 0), "a power weapon's four");
        assert!(game.feast.pickups[0].ready > game.feast.elapsed);
    }
}
#[test]
fn both_ramps_reach_the_turret_deck_on_foot() {
    for (a, b) in [([0.55f32, 3.53f32], [1.4f32, 3.53f32]), ([1.65, 2.42], [1.65, 3.3])] {
        let mut s = still();
        s.feast.players[0].p = [a[0], FLOOR, a[1]];
        s.tick(STEP);
        let steps = ((b[0] - a[0]).hypot(b[1] - a[1]) / (WALK / HZ) as f32) as usize;
        play(&mut s, 1, &walking(1, steps, Vec2::new(0., -1.), facing(a, b), 0, 0));
        assert!((at(&s, 1)[1] - (FLOOR + 0.28)).abs() < 1e-4, "on the deck: {:?}", at(&s, 1));
    }
}
#[test]
fn disconnected_humans_are_replaced_by_bots_and_rounds_reset_cleanly() {
    let mut s = Game::new(read_map("assets/maps/counter-siege.json"), &Settings { demo: true, ..Settings::default() });
    let p = s.join().unwrap();
    let q = s.join().unwrap();
    s.leave(q);
    assert_eq!(s.players(), 5);
    assert!(s.player(q).unwrap().bot && !s.connected(q));
    s.feast.players[p as usize - 1].hp = 4.;
    s.feast.players[p as usize - 1].consumed = true;
    s.leave(p);
    // Nobody is left: nothing is listed, and a new round waits for whoever comes next.
    assert_eq!(s.players(), 0);
    let again = s.join().unwrap();
    let hotdog = s.player(again).unwrap();
    assert_eq!((hotdog.hp, hotdog.consumed, s.feast.stage), (100., false, Stage::Warmup));
    assert!(s.feast.enemies.is_empty());
}
#[test]
fn a_persons_walk_shoves_a_hotdog_out_of_the_way_rather_than_through_it() {
    let mut s = still();
    s.feast.players[0].p = [0., FLOOR, 2.4];
    s.feast.players[1].p = [0.09, FLOOR, 2.4];
    s.tick(STEP);
    let gap = |s: &Game| {
        let (a, b) = (at(s, 1), at(s, 2));
        (a[0] - b[0]).hypot(a[2] - b[2]) - 0.09
    };
    let before = (at(&s, 1)[0], at(&s, 2)[0]);
    for c in walking(1, 30, Vec2::new(0., -1.), -std::f32::consts::FRAC_PI_2, 0, 0) {
        play(&mut s, 1, &[c]);
        assert!(gap(&s) >= -1e-6, "overlap of {}", -gap(&s));
    }
    let (a, b) = (at(&s, 1)[0], at(&s, 2)[0]);
    assert!(a > before.0 + 0.1 && b > before.1 + 0.1 && a < b, "the other hotdog gave way: {before:?} -> {a} {b}");
}
#[test]
fn a_block_the_map_has_stops_a_walk() {
    // A block that only the busier map has, between z = 2.66 and 2.86: it stops a hotdog walking into it.
    for (path, blocked) in [("assets/maps/counter-siege.json", false), ("assets/maps/counter-siege-v2.json", true)] {
        let mut s = round_on(read_map(path), Settings::default());
        for q in s.feast.players.iter_mut().skip(1) {
            (q.bot, q.p) = (false, [-1.6 + q.id as f32 * 0.1, FLOOR, 3.55]);
        }
        s.feast.players[0].p = [-0.01, FLOOR, 2.56];
        s.tick(STEP);
        play(&mut s, 1, &walking(1, 30, Vec2::new(0., -1.), std::f32::consts::PI, 0, 0));
        assert_eq!(at(&s, 1)[2] > 2.7, !blocked, "{path}");
        assert_eq!(at(&s, 1)[2] < 2.66 - 0.029, blocked, "{path}");
    }
}
#[test]
fn a_crouch_walks_at_65_percent_of_the_pace_prone_at_40_and_the_stance_is_in_the_frame() {
    let mut s = still();
    let walk = |s: &mut Game, first: u32, held: u16| {
        s.feast.players[0].p = [0.1, FLOOR, 2.4];
        s.tick(STEP);
        let from = at(s, 1);
        play(s, 1, &walking(first, 30, Vec2::new(0., -1.), -std::f32::consts::FRAC_PI_2, held, 0));
        at(s, 1)[0] - from[0]
    };
    let standing = walk(&mut s, 1, 0);
    let crouched = walk(&mut s, 31, button::CROUCH);
    assert!((crouched / standing - 0.65).abs() < 0.01, "{crouched} {standing}");
    assert_eq!(frame_json(&s)["players"][0]["crouch"], true);
    let prone = walk(&mut s, 61, button::PRONE | button::CROUCH);
    assert!((prone / standing - 0.4).abs() < 0.01, "{prone} {standing}");
    let me = &frame_json(&s)["players"][0];
    assert_eq!((me["prone"].clone(), me["crouch"].clone()), (json!(true), json!(false)), "prone wins");
}
#[test]
fn each_round_of_a_syrup_burst_leaves_along_the_look_of_the_command_applied_before_it() {
    let mut s = still();
    // The trigger along -z, then the look turned a hundredth of a radian a step: each round leaves along the yaw of the
    // latest command when it leaves, not the first round's.
    let yaw = |k: u32| 0.01 * k as f32;
    let commands: Vec<Command> = (0..15).map(|k| Command::new(1 + k, Vec2::ZERO, yaw(k), 0., if k == 0 { button::FIRE } else { 0 }, RIFLE as u8, None)).collect();
    let (mut ways, mut last) = (vec![], 0);
    for c in &commands {
        play(&mut s, 1, std::slice::from_ref(c));
        let since = last;
        for shot in s.feast.shots.iter().filter(|shot| shot.owner == 1 && shot.weapon == RIFLE && shot.id > since) {
            let way = Vec3::from_array(shot.b) - Vec3::from_array(shot.a);
            ways.push(((-way.x).atan2(-way.z), c.yaw()));
            last = shot.id;
        }
    }
    assert_eq!(ways.len(), 3, "{ways:?}");
    for (k, (went, looked)) in ways.iter().enumerate() {
        assert!((went - looked).abs() < 1e-3, "round {k} left along {went}, the look was {looked}");
    }
    assert!(ways[2].0 > ways[1].0 + 0.04 && ways[1].0 > ways[0].0 + 0.04, "{ways:?}");
}
#[test]
fn aiming_down_the_sights_reaches_the_server_halves_a_syrup_bursts_climb_is_in_the_frame_and_goes_with_the_rifle() {
    let mut s = still();
    let north = 0.;
    // One burst along -z, across the counter, the rifle in hand, `held` held throughout: how much each round rises, in order.
    let burst = |s: &mut Game, first: u32, held: u16| -> Vec<f32> {
        let start = s.feast.elapsed;
        let mut commands = vec![Command::new(first, Vec2::ZERO, north, 0., held | button::FIRE, RIFLE as u8, None)];
        commands.extend((1..15).map(|k| Command::new(first + k, Vec2::ZERO, north, 0., held, RIFLE as u8, None)));
        // (Each round's rise as it leaves: the burst lasts 0.2 s, and the first round has flown its 4 m by its end.)
        let (mut rises, mut last) = (vec![], 0);
        for c in &commands {
            play(s, 1, std::slice::from_ref(c));
            let mut shots: Vec<_> = s.feast.shots.iter().filter(|shot| shot.owner == 1 && shot.weapon == RIFLE && shot.time >= start - 1e-6 && shot.id > last).collect();
            shots.sort_by_key(|shot| shot.id);
            for shot in shots {
                last = shot.id;
                if let Some(q) = s.feast.projectiles.iter().find(|q| q.id == shot.projectile) {
                    rises.push(q.v[1] / Vec3::from_array(q.v).length());
                }
            }
        }
        // (Past the burst's pause before the next.)
        play(s, 1, &(first + 15..first + 40).map(|k| Command::new(k, Vec2::ZERO, north, 0., held, RIFLE as u8, None)).collect::<Vec<_>>());
        rises
    };
    let hip = burst(&mut s, 1, 0);
    assert!(!s.player(1).unwrap().aimed && frame_json(&s)["players"][0]["aimed"] == json!(false));
    let aimed = burst(&mut s, 41, button::SCOPE);
    assert!(s.player(1).unwrap().aimed, "SCOPE held with the rifle in hand aims it");
    assert_eq!((hip.len(), aimed.len()), (3, 3), "{hip:?} {aimed:?}");
    for k in 1..3 {
        assert!((hip[k] - 0.004 * k as f32).abs() < 1e-4 && (aimed[k] - 0.002 * k as f32).abs() < 1e-4, "round {k}: {} at the hip, {} aimed", hip[k], aimed[k]);
    }
    // Every client sees it: the frame says so (bit 13 of a hotdog's flags in a binary frame).
    assert_eq!(frame_json(&s)["players"][0]["aimed"], json!(true));
    assert_ne!(s.frame().state().tables[kitchen_sim::net::PLAYERS][0].ints[9] & 1 << 13, 0);
    // Let go, or the pistol in hand (no sights to aim down), and it is gone.
    play(&mut s, 1, &[Command::new(81, Vec2::ZERO, north, 0., button::SCOPE, 1, None)]);
    assert!(!s.player(1).unwrap().aimed);
    play(&mut s, 1, &[Command::new(82, Vec2::ZERO, north, 0., button::SCOPE, RIFLE as u8, None), Command::new(83, Vec2::ZERO, north, 0., 0, RIFLE as u8, None)]);
    assert!(!s.player(1).unwrap().aimed);
    assert_eq!(frame_json(&s)["players"][0]["aimed"], json!(false));
    // The debug encoding's `scope` is the same bit.
    let c = crate::protocol::commands(&json!({"commands": [{"seq": 1, "scope": true, "fire": 1}]}));
    assert_eq!(c[0].buttons, button::SCOPE | button::FIRE);
}
/// How a carried object is held travels in the commands (`button::TURN` and `button::TIP`): a Forts hotdog that picks up
/// the top cutting board of its side's pile, tips it onto its long edge and puts it down has it standing there as a
/// wall panel 30 cm tall, and the frame says so. Picked up again, it is held flat again until the commands say not.
#[test]
fn how_a_carried_object_is_held_travels_in_the_commands_and_it_lands_as_it_was_held() {
    use kitchen_sim::feast::build::Pose;
    let mut s = round_on(read_map("assets/maps/pillow-fort.json"), Settings::default());
    for q in s.feast.players.iter_mut() {
        q.bot = false;
    }
    s.feast.set_build(600.);
    // Hotdog 1 in front of its pile of three boards (at [-2.2, 2.4]), facing it (-z), looking down at it.
    s.feast.players[0].p = [-2.2, FLOOR, 2.64];
    s.tick(STEP);
    let (north, down) = (0., -0.7);
    play(&mut s, 1, &[Command::new(1, Vec2::ZERO, north, down, button::GRAB, 0, None), Command::new(2, Vec2::ZERO, north, down, 0, 0, None)]);
    let board = s.player(1).unwrap().carry;
    assert_eq!(board, 2, "the top board");
    // Tipped onto its long edge, held still a moment, then put down.
    let tipped = Command::posed(Pose { turn: 0, tip: 1 });
    play(&mut s, 1, &(3..8).map(|k| Command::new(k, Vec2::ZERO, north, 0., tipped, 0, None)).collect::<Vec<_>>());
    assert_eq!(s.player(1).unwrap().pose, Pose { turn: 0, tip: 1 });
    play(&mut s, 1, &[Command::new(8, Vec2::ZERO, north, 0., tipped | button::GRAB, 0, None)]);
    play(&mut s, 1, &(9..70).map(|k| Command::new(k, Vec2::ZERO, north, 0., tipped, 0, None)).collect::<Vec<_>>());
    let board = board as usize;
    assert_eq!(s.player(1).unwrap().carry, -1, "put down");
    let e = s.feast.props.extent(board);
    assert!((e[5] - e[4] - 0.30).abs() < 0.004 && s.feast.props.up(board)[1].abs() < 1e-3 && s.feast.props.state(board) == kitchen_sim::props::State::Locked, "standing on its long edge: {e:?}");
    let shown = frame_json(&s)["props"].as_array().unwrap().iter().find(|p| p["id"] == json!(board)).cloned().unwrap();
    let q = s.feast.props.rotation(board);
    assert!((0..4).all(|k| (shown["q"][k].as_f64().unwrap() - q[k]).abs() < 1e-4), "the frame has it as it stands: {shown}");
    // Picked up again with no pose in the commands: held flat.
    play(&mut s, 1, &[Command::new(70, Vec2::ZERO, north, down, button::GRAB, 0, None), Command::new(71, Vec2::ZERO, north, down, 0, 0, None)]);
    assert_eq!((s.player(1).unwrap().carry, s.player(1).unwrap().pose), (board as i32, Pose::default()));
    assert!(s.feast.props.up(board)[1] > 0.999, "held flat");
    // The debug encoding says the same with `turn` and `tip`.
    let c = crate::protocol::commands(&json!({"commands": [{"seq": 1, "turn": 3, "tip": 2, "grab": 1}]}));
    assert_eq!((c[0].pose(), c[0].has(button::GRAB)), (Pose { turn: 3, tip: 2 }, true));
}
#[test]
fn nothing_selects_bare_hands_and_aim_pitch_follows_every_command() {
    let mut s = fresh();
    let mut seq = 0;
    for weapon in [json!(0), json!(2), json!(7), json!(-1), json!("1")] {
        seq += 1;
        let commands = crate::protocol::commands(&json!({"type": "input", "commands": [{"seq": seq, "weapon": weapon}]}));
        play(&mut s, 1, &commands);
        let p = s.player(1).unwrap();
        assert_eq!((p.weapon, p.gun), (RIFLE, RIFLE), "asked for {weapon}");
    }
    assert_eq!(s.player(1).unwrap().pitch, 0.);
    play(&mut s, 1, &[Command::new(6, Vec2::ZERO, 0., 0.6, 0, 1, None)]);
    assert!((s.player(1).unwrap().pitch - 0.6).abs() < 1e-4);
    play(&mut s, 1, &[Command::new(7, Vec2::ZERO, 0., -5., 0, 1, None)]);
    assert!((s.player(1).unwrap().pitch + 1.5).abs() < 1e-4, "held to 1.5 radians");
    let snap = frame_json(&s);
    let me = &snap["players"][0];
    assert!((me["pitch"].as_f64().unwrap() + 1.5).abs() < 1e-4);
    // What the server keeps of a hotdog for itself stays out of the frame.
    for private in ["direction", "cooldown", "bite_cooldown", "camp", "camped", "errand", "errand_at", "shun", "job", "pace", "last", "volley", "volley_wait", "volley_aim", "connected", "last_move", "queue", "budget"] {
        assert!(me.get(private).is_none(), "{private} is not sent");
    }
    // An aimed shot goes along the command's aim.
    play(&mut s, 1, &[Command::new(8, Vec2::ZERO, std::f32::consts::PI, 0.6, button::FIRE, 1, None)]);
    assert_eq!(s.player(1).unwrap().ammo[1], 11);
    assert!((s.player(1).unwrap().pitch - 0.6).abs() < 1e-4);
    let shot = s.feast.shots.iter().find(|shot| shot.owner == 1).expect("a shot");
    let along = (Vec3::from_array(shot.b) - Vec3::from_array(shot.a)).normalize();
    assert!(along.distance(Vec3::new(0., 0.6f32.sin(), 0.6f32.cos())) < 1e-3, "{along}");
}
#[test]
fn commands_are_applied_in_order_one_a_step_repeats_and_ones_far_ahead_are_passed_over_and_a_backlog_is_caught_up() {
    let mut s = still();
    let c = |seq: u32| Command::new(seq, Vec2::ZERO, 0., 0., 0, 0, None);
    s.commands(1, &[c(1)]);
    s.tick(STEP);
    assert_eq!(s.applied(1), 1);
    // A batch repeats the commands before its new one: those are passed over.
    s.commands(1, &[c(1), c(2)]);
    s.tick(STEP);
    assert_eq!((s.applied(1), s.waiting_commands(1)), (2, 0));
    // Four arrive at once (a stall on the way): two a step while more than two wait, then one a step.
    s.commands(1, &[c(2), c(3), c(4), c(5), c(6)]);
    let mut after = vec![];
    for _ in 0..3 {
        s.tick(STEP);
        after.push(s.applied(1));
    }
    assert_eq!(after, [4, 5, 6]);
    // Late (older than the newest it has) or far ahead: passed over. A gap is taken (what was missed is gone).
    s.commands(1, &[c(5)]);
    s.commands(1, &[c(6 + crate::game::AHEAD + 1)]);
    assert_eq!(s.waiting_commands(1), 0);
    s.commands(1, &[c(9)]);
    s.tick(STEP);
    assert_eq!(s.applied(1), 9);
    // A flood (a client sending faster than time): at most QUEUE wait, the oldest going first with what was pressed
    // in them kept, and over time no more than a command a step (and a little) is applied.
    let mut flood: Vec<Command> = (10..10 + 60).map(c).collect();
    flood[0].buttons = button::STIM;
    s.commands(1, &flood);
    assert_eq!(s.waiting_commands(1), crate::game::QUEUE);
    let mut applied = vec![s.applied(1)];
    for _ in 0..20 {
        s.tick(STEP);
        applied.push(s.applied(1));
    }
    let gained: Vec<u32> = applied.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gained.iter().all(|n| *n <= 2), "{gained:?}");
    assert!(applied[20] - applied[0] <= (crate::game::BUDGET + 20. * crate::game::RATE) as u32 + 1, "{applied:?}");
    assert!(s.player(1).unwrap().stim > 0., "the stim pressed in a command that was dropped was kept");
}
/// The stage-1 server took a client's claims of where it stood and refused one that dropped more than 0.14 m (off a
/// ramp's side); it then stopped taking that client's claims, and the client, finding the server's place on its own
/// trail, never put itself right: it was stranded. Now the client sends what it does, and the server walks it there.
#[test]
fn walking_off_a_ramps_side_is_followed_step_by_step_and_the_client_predicts_the_same_fall() {
    let mut s = still();
    let ramp = [1.65f32, 2.40f32];
    s.feast.players[0].p = [ramp[0], FLOOR, ramp[1]];
    s.tick(STEP);
    // The client's own copy: its props, and its hotdog's body as the server has it.
    let mut client = Feast::with_map(read_map("assets/maps/counter-siege.json"), false);
    for _ in 0..(3. * HZ) as usize {
        client.props.step(&client.level, STEP as f64);
    }
    let mut ghosts = kitchen_sim::actor::World::default();
    let mut body = kitchen_sim::actor::Actor { radius: kitchen_sim::feast::HOTDOG_RADIUS, ..kitchen_sim::actor::Actor::hotdog(1, [0.; 3]) };
    kitchen_sim::net::Mine::of(s.feast.body(1).unwrap()).onto(&mut body);
    ghosts.add(body);
    // 0.45 m up the ramp, then off its side.
    let climb = (0.45 / (WALK / HZ)).round() as usize;
    let up = walking(1, climb, Vec2::new(0., -1.), std::f32::consts::PI, 0, 0);
    // (A few steps sideways take it off: then it stands still and falls straight down, clear of the bowl beside the ramp
    // that a walk carried on at 1.25 m/s would land on.)
    let side = walking(climb as u32 + 1, 8, Vec2::new(1., 0.), std::f32::consts::PI, 0, 0);
    let fall = walking(climb as u32 + 9, 22, Vec2::ZERO, std::f32::consts::PI, 0, 0);
    let mut lowest = f32::MAX;
    let mut highest = f32::MIN;
    for c in up.iter().chain(&side).chain(&fall) {
        let step = c.stride(kitchen_sim::net::pace(false, false, false, 1.), STEP);
        client.predict(&mut ghosts, 1, (false, false), step, 0., STEP as f64, true);
        play(&mut s, 1, std::slice::from_ref(c));
        let (server, predicted) = (Vec3::from_array(at(&s, 1)), ghosts.get(1).unwrap().position());
        assert!(server.distance(predicted) < 1e-5, "command {}: the server has {server}, the client predicted {predicted}", c.seq);
        assert_eq!(s.applied(1), c.seq);
        if c.seq as usize <= climb {
            highest = highest.max(server.y);
        } else {
            lowest = lowest.min(server.y);
        }
    }
    assert!(highest - FLOOR > 0.14, "up the ramp more than 0.14 m: {}", highest - FLOOR);
    assert!((lowest - FLOOR).abs() < 1e-4 && at(&s, 1)[0] < 1.55, "off its side and down on the counter: {:?}", at(&s, 1));
}
#[test]
fn every_frame_has_every_loose_object_and_a_delta_carries_one_only_while_it_moves() {
    use kitchen_sim::net::{PROPS, Reader, State, Writer, write_state};
    // The changes between two frames' states: which tables changed (net.rs `write_state`'s mask).
    let changed = |a: &State, b: &State| {
        let mut w = Writer::new();
        write_state(&mut w, Some(a), b);
        Reader::new(&w.bytes).varint().unwrap()
    };
    let mut joined = Game::new(read_map("assets/maps/counter-siege.json"), &Settings { demo: true, ..Settings::default() });
    joined.join();
    let props = frame_json(&joined)["props"].clone();
    assert_eq!(props.as_array().unwrap().len(), 1);
    assert_eq!((props[0]["id"].clone(), props[0]["p"].clone()), (json!(0), json!([1.2, kitchen_sim::map::FLOOR, 2.75])));
    let mut s = still();
    for _ in 0..(1.5 * HZ) as usize {
        s.tick(STEP);
    }
    let rest = s.frame().state();
    s.tick(STEP);
    assert_eq!(changed(&rest, &s.frame().state()) & 1 << PROPS, 0, "a sleeping bowl is not sent again");
    // Shoved, it is sent while it moves.
    s.feast.props.apply_impulse(0, [0.05, 0., 0.], None);
    s.tick(STEP);
    let moving = s.frame();
    assert_ne!(changed(&rest, &moving.state()) & 1 << PROPS, 0);
    assert!(moving.props[0].state == 0 && moving.props[0].v[0] > 0.05);
    for _ in 0..(4. * HZ) as usize {
        s.tick(STEP);
    }
    let settled = s.frame().state();
    s.tick(STEP);
    assert_eq!(changed(&settled, &s.frame().state()) & 1 << PROPS, 0);
    assert!(s.feast.props.position(0)[0] > 1.2);
}
/// The client predicts its own hotdog shoving the bowl with its own copy of the props; the server, applying the same
/// commands, shoves its bowl the same way and ends with the hotdog where the client had it.
#[test]
fn a_persons_hotdog_shoves_the_bowl_through_the_server_as_its_own_prediction_does() {
    let mut server = still();
    server.feast.players[0].p = [1.45, FLOOR, 2.7];
    server.tick(STEP);
    let mut client = Feast::with_map(read_map("assets/maps/counter-siege.json"), false);
    for _ in 0..(3. * HZ) as usize {
        client.props.step(&client.level, STEP as f64);
    }
    let mut ghosts = kitchen_sim::actor::World::default();
    let mut body = kitchen_sim::actor::Actor { radius: kitchen_sim::feast::HOTDOG_RADIUS, ..kitchen_sim::actor::Actor::hotdog(1, [0.; 3]) };
    kitchen_sim::net::Mine::of(server.feast.body(1).unwrap()).onto(&mut body);
    ghosts.add(body);
    let yaw = facing([0., 0.], [-0.98, 0.199]);
    let apart = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
    let mut worst = 0f32;
    for c in walking(1, 120, Vec2::new(0., -1.), yaw, 0, 0) {
        let step = c.stride(kitchen_sim::net::pace(false, false, false, 1.), STEP);
        client.predict(&mut ghosts, 1, (false, false), step, 0., STEP as f64, true);
        play(&mut server, 1, &[c]);
        worst = worst.max(Vec3::from_array(at(&server, 1)).distance(ghosts.get(1).unwrap().position()));
    }
    let (here, there) = (client.props.position(0), server.feast.props.position(0));
    assert!(apart(here, [1.2, 0., 2.75]) > 0.4, "the client shoved its bowl {} m", apart(here, [1.2, 0., 2.75]));
    assert!(apart(there, [1.2, 0., 2.75]) > 0.4, "and the server its own {} m", apart(there, [1.2, 0., 2.75]));
    assert!(apart(here, there) < 0.01 && worst < 0.002, "the bowls {} m apart, the hotdogs at worst {worst} m", apart(here, there));
}
#[test]
fn while_the_forcefield_stands_a_walk_stops_at_it_however_far_and_one_on_its_own_side_is_taken() {
    let mut s = round_on(fort_map(), Settings { build: Some(30.), ..Settings::default() });
    for q in s.feast.players.iter_mut() {
        q.bot = false;
    }
    assert!(s.feast.fort.as_ref().unwrap().up);
    s.feast.players[0].p = [-0.1, FLOOR, 2.5];
    s.tick(STEP);
    play(&mut s, 1, &walking(1, 60, Vec2::new(0., -1.), -std::f32::consts::FRAC_PI_2, 0, 0));
    let x = at(&s, 1)[0];
    assert!(x < -0.069 && x > -0.12, "stopped at {x}");
    play(&mut s, 1, &walking(61, 9, Vec2::new(0., -1.), std::f32::consts::FRAC_PI_2, 0, 0));
    assert!((at(&s, 1)[0] - (x - (9. * WALK / HZ) as f32)).abs() < 1e-3, "a walk on its own side is taken: {}", at(&s, 1)[0]);
}
#[test]
fn a_forts_snapshot_carries_each_hotdogs_team_and_load_and_the_modes_state_and_a_siege_has_none() {
    let mut s = round_on(fort_map(), Settings { build: Some(5.), ..Settings::default() });
    let snap = frame_json(&s);
    let players = snap["players"].as_array().unwrap();
    assert_eq!(players.iter().map(|q| json!([q["team"], q["carry"]])).collect::<Vec<_>>(), (0..6).map(|i| json!([i % 2, -1])).collect::<Vec<_>>());
    assert!(players[0].get("job").is_none());
    assert_eq!(players.iter().map(|q| q["name"].as_str().unwrap()).collect::<Vec<_>>()[1..], ["TOMATO", "HONEY", "CATSUP", "GRAINY", "PASSATA"]);
    let forts = &snap["forts"];
    assert_eq!(
        json!([snap["stage"], forts["scores"], forts["rounds"], forts["field"], forts["build"], forts["ends"], forts["bacon"]["state"], forts["stale"]]),
        json!(["Build", [0, 0], forts::ROUNDS, true, 5.0, 5.0, forts::HOME, forts::STALE])
    );
    // A seventh is refused.
    for _ in 0..5 {
        assert!(s.join().is_some());
    }
    assert_eq!(s.join(), None);
    // --stale sets the quiet seconds before the cats come.
    let quiet = round_on(fort_map(), Settings { build: Some(5.), stale: Some(12.), ..Settings::default() });
    assert_eq!(quiet.feast.fort_state().unwrap().stale, 12.);
    let siege = frame_json(&fresh());
    assert!(siege.get("forts").is_none());
    assert_eq!(siege["players"].as_array().unwrap().len(), 5);
}
#[test]
fn a_forts_match_won_returns_to_the_lobby_with_nobody_ready_and_the_next_match_from_nothing() {
    let mut g = Game::new(fort_map(), &Settings { build: Some(5.), countdown: 1., opening: 1., ..Settings::default() });
    let p = g.join().unwrap();
    assert_eq!(g.stage(), "Lobby");
    g.ready(p, true);
    for _ in 0..(2.2 * HZ) as usize {
        g.tick(STEP);
    }
    assert_eq!(g.stage(), "Build");
    for round in 0..3 {
        for _ in 0..(5.1 * HZ) as usize {
            g.tick(STEP);
        }
        assert_eq!(g.stage(), "Fight", "round {round}");
        let carrier = g.feast.players.iter().position(|q| q.team == 1).unwrap();
        g.feast.players[carrier].p = [0., FLOOR + 0.12, 3.1];
        g.tick(STEP);
        g.feast.players[carrier].p = [2.75, FLOOR, 3.1];
        g.tick(STEP);
        assert_eq!(g.feast.stage, Stage::RoundOver);
        for _ in 0..((INTERMISSION as f64 + 0.1) * HZ) as usize {
            g.tick(STEP);
        }
        assert_eq!(g.stage(), if round < 2 { "Build" } else { "Lobby" });
    }
    let fort = g.feast.fort.as_ref().unwrap();
    assert_eq!((fort.scores, fort.champion, fort.match_no, g.player(p).unwrap().ready), ([0, 0], None, 2, false));
}
#[test]
fn snapshots_carry_what_last_hurt_each_hotdog_the_pickups_and_the_emergences() {
    let mut s = still();
    s.feast.emerge(2);
    for _ in 0..(4. * HZ) as usize {
        s.tick(STEP);
    }
    let snap = frame_json(&s);
    assert_eq!(snap["emergences"].as_array().unwrap().len(), 1);
    assert_eq!(snap["emergences"][0]["total"], 2);
    let pickup = &snap["pickups"][0];
    for key in ["id", "p", "weapon", "ammo", "ready", "respawn"] {
        assert!(pickup.get(key).is_some(), "a pickup's {key}");
    }
    let me = &snap["players"][0];
    assert_eq!(me["last_damage"], json!({"kind": 0, "attacker": 0, "p": [0.0, 0.0, 0.0], "d": [0.0, 0.0, 0.0], "time": 0.0, "amount": 0.0}));
    for key in ["v", "grounded", "launched", "fallen", "swings", "pitch", "carry", "ammo", "owned", "human", "waiting", "talk", "color", "ready"] {
        assert!(me.get(key).is_some(), "a hotdog's {key}");
    }
    assert_eq!(snap["turrets"].as_array().unwrap().len(), s.feast.turret_yaw.len());
    assert!(!s.feast.turret_yaw.is_empty(), "the bundled map has a turret");
    assert!(snap["drapes"].as_array().unwrap().is_empty() && snap["knocks"].is_array());
    // A zombie's body has its own id among the bodies.
    assert!(s.feast.enemies.iter().all(|e| e.forming > 0. || s.feast.body(CAT + e.id).is_some()));
    // The frame in whole numbers and back: everything to within its unit (net.rs), the emergence exactly.
    let frame = s.frame();
    let back = kitchen_sim::net::Frame::from_state(&frame.state(), frame.elapsed);
    for (a, b) in frame.players.iter().zip(&back.players) {
        assert!(Vec3::from_array(a.player.p).distance(Vec3::from_array(b.player.p)) < 0.001, "{:?} {:?}", a.player.p, b.player.p);
    }
    assert_eq!(serde_json::to_value(&back.emergences[0].schedule).unwrap().as_array().unwrap().len(), frame.emergences[0].schedule.len());
    assert_eq!((back.emergences[0].total, back.emergences[0].puddles.len()), (frame.emergences[0].total, frame.emergences[0].puddles.len()));
}
#[test]
fn a_melee_blow_from_behind_arrives_as_a_backstab_on_the_victims_record_in_every_form_of_the_frame() {
    use kitchen_sim::feast::BACKSTAB;
    // The protocol and its frames are as they were: the backstab is a damage kind on the record that was always there.
    assert_eq!(kitchen_sim::net::PROTOCOL, 12);
    // Hotdog 1 (a person) strikes with F, as a client sends it, at hotdog 2, 0.14 m ahead of it and facing away or
    // facing it; the blow is made the way a command's yaw and pitch aim it.
    for (turn, kills) in [(0., true), (std::f32::consts::PI, false)] {
        let mut s = still();
        s.feast.stage = Stage::FreeForAll;
        s.feast.players[0].p = [0., FLOOR, 2.4];
        s.feast.players[1].p = [0., FLOOR, 2.54];
        // (Hotdog 1 looks along +z, which a yaw of pi is; hotdog 2 looks along +z too, or back at it.)
        s.feast.players[1].yaw = std::f32::consts::PI + turn;
        play(&mut s, 1, &[Command::new(1, Vec2::ZERO, std::f32::consts::PI, 0., 0, 1, None), Command::new(2, Vec2::ZERO, std::f32::consts::PI, 0., button::MELEE, 1, None)]);
        let victim = s.player(2).unwrap();
        assert_eq!(victim.alive, !kills, "{} HP", victim.hp);
        if !kills {
            // From the front it took half of 100 (the hunger of the free for all, which is on the record by now, a little
            // more), and the record has the next step's hunger on it: the blow itself is in `swings` and the HP.
            assert!(victim.hp > 49. && victim.hp < 50.5, "{} HP", victim.hp);
            assert_eq!(s.player(1).unwrap().swings, 1);
            continue;
        }
        assert_eq!((victim.last_damage.kind, victim.last_damage.attacker), (BACKSTAB, 1));
        // On the wire: as JSON, as whole numbers and back.
        let snap = frame_json(&s);
        assert_eq!(snap["players"][1]["last_damage"]["kind"], json!(victim.last_damage.kind));
        assert_eq!(snap["players"][1]["last_damage"]["attacker"], 1);
        let frame = s.frame();
        let back = kitchen_sim::net::Frame::from_state(&frame.state(), frame.elapsed);
        let told = &back.players[1].player.last_damage;
        assert_eq!((told.kind, told.attacker), (victim.last_damage.kind, 1));
        assert!((told.amount - victim.last_damage.amount).abs() < 0.01, "the HP it took, to a hundredth");
        assert_eq!(back.players[1].player.alive, !kills);
    }
}
#[test]
fn a_hotdog_that_leaves_mid_round_is_played_on_by_a_bot_and_when_nobody_seated_is_left_the_game_goes_back_to_the_lobby() {
    let mut s = Game::new(read_map("assets/maps/counter-siege.json"), &Settings { countdown: 0., opening: 0., ..Settings::default() });
    let (a, b) = (s.join().unwrap(), s.join().unwrap());
    s.ready(a, true);
    s.ready(b, true);
    s.tick(STEP);
    s.tick(STEP);
    assert_eq!(s.stage(), "Warmup");
    for _ in 0..10 {
        s.tick(STEP);
    }
    play(&mut s, b, &[Command::new(1, Vec2::ZERO, std::f32::consts::PI, 0., button::CROUCH | button::FIRE, 1, None)]);
    let (was, ammo) = (at(&s, b), s.player(b).unwrap().ammo[1]);
    assert!(s.player(b).unwrap().crouch && ammo == 11);
    s.leave(b);
    let hotdog = s.player(b).unwrap();
    assert!(hotdog.bot && !hotdog.crouch && !s.connected(b));
    assert_eq!((hotdog.p, hotdog.ammo[1]), (was, ammo), "the bot goes on from where the hotdog stood, with what it had");
    assert_eq!(s.stage(), "Warmup");
    // Nobody seated is left: back to the lobby, with a fresh round and nobody listed.
    s.leave(a);
    assert_eq!((s.stage(), s.players()), ("Lobby", 0));
    assert!(s.feast.players.iter().all(|p| p.bot && p.hp == 100.));
}

// ----- The game loop: its steps, and the frames each game socket is sent.

#[test]
fn the_loop_takes_every_step_it_owes_at_once_up_to_a_limit_and_lets_the_rest_go() {
    use crate::hub::{CATCH_UP, owed};
    use std::time::Duration;
    let step = Duration::from_micros(16_667);
    let t0 = std::time::Instant::now();
    // On time: the step that is due.
    assert_eq!(owed(t0 + step, t0 + step, step), (1, t0 + step * 2, None));
    // Late by three and a bit: four steps now, and the next one due on the old beat.
    let (n, next, lost) = owed(t0 + step, t0 + step * 4 + step / 3, step);
    assert_eq!((n, next, lost), (4, t0 + step * 5, None));
    // Stalled for a second: CATCH_UP steps, and the rest of the time let go (and said).
    let (n, next, lost) = owed(t0 + step, t0 + Duration::from_secs(1), step);
    assert_eq!(n, CATCH_UP);
    assert!(lost.is_some_and(|l| l > Duration::from_millis(700)) && next == t0 + Duration::from_secs(1) + step);
    // Early: nothing yet.
    assert_eq!(owed(t0 + step, t0, step).0, 0);
}
/// A game socket (binary) through the hub: what the client is sent and what it makes of it, as src/network.rs
/// decodes it.
struct Client {
    inbox: Inbox,
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    read: usize,
    states: Vec<(u32, kitchen_sim::net::State)>,
    bases: Vec<u32>,
}
impl Client {
    /// Decodes the frames that came since the last call against what it has; returns their steps.
    fn take(&mut self) -> Vec<u32> {
        let frames: Vec<Vec<u8>> = self.frames.lock().unwrap()[self.read..].to_vec();
        self.read += frames.len();
        let mut ticks = vec![];
        for bytes in frames {
            let (tick, base) = kitchen_sim::net::peek_frame(&bytes).unwrap();
            let state = self.states.iter().find(|(t, _)| *t == base).map(|(_, s)| s);
            assert!(base == 0 || state.is_some(), "a frame against {base}, which the client never had");
            let (header, state, _) = kitchen_sim::net::decode_frame(&bytes, state).unwrap();
            assert_eq!(state.checksum(), header.checksum, "the frame of {tick} decodes to what the server has");
            self.states.push((tick, state));
            self.bases.push(base);
            ticks.push(tick);
        }
        ticks
    }
}
#[test]
fn a_game_socket_is_sent_frames_against_what_it_acknowledged_whole_until_then_and_whole_again_when_it_asks() {
    use crate::hub::{Event, Hub};
    use kitchen_sim::net::Batch;
    let t = Table::plain();
    let mut hub = Hub::new(t.lobby, crate::config::Net::default());
    let (outbox, inbox, frames) = Outbox::memories();
    hub.event(Event::Open { peer: 1, page: false, outbox });
    hub.event(Event::Message { peer: 1, message: json!({"type": "join", "version": 12, "name": "Nell"}) });
    let welcome: Value = serde_json::from_str(&inbox.lock().unwrap()[0]).unwrap();
    assert_eq!((welcome["type"].clone(), welcome["tick"].clone(), welcome["send"].clone()), (json!("welcome"), json!(60), json!(30)));
    let mut client = Client { inbox, frames, read: 0, states: vec![], bases: vec![] };
    let frame = |hub: &mut Hub| {
        while !hub.advance() {}
        hub.send();
    };
    let ack = |hub: &mut Hub, tick: u32, resync: bool| {
        hub.event(Event::Binary { peer: 1, bytes: Batch { ack: tick, resync, time: 0, commands: vec![] }.encode() });
    };
    // The join's own frame, and every frame until one is acknowledged: whole.
    frame(&mut hub);
    frame(&mut hub);
    let first = client.take();
    assert_eq!(first.len(), 3);
    assert_eq!(client.bases, [0, 0, 0]);
    // Acknowledged: the next is a delta against it, and smaller.
    ack(&mut hub, first[1], false);
    frame(&mut hub);
    let next = client.take();
    assert_eq!(client.bases[3], first[1]);
    let sizes: Vec<usize> = client.frames.lock().unwrap().iter().map(|f| f.len()).collect();
    assert!(sizes[3] < sizes[2], "{sizes:?}");
    // Acks that never come (dropped): the frames stay against the last one acknowledged, which the client still has.
    for _ in 0..4 {
        frame(&mut hub);
    }
    client.take();
    assert!(client.bases[4..].iter().all(|b| *b == first[1]), "{:?}", client.bases);
    // An ack for a frame never sent is not taken.
    ack(&mut hub, next[0] + 1, false);
    frame(&mut hub);
    client.take();
    assert_eq!(*client.bases.last().unwrap(), first[1]);
    // The client asks for a whole frame (its decoding went wrong): it gets one, then deltas again once it acks.
    ack(&mut hub, next[0], true);
    frame(&mut hub);
    let whole = client.take();
    assert_eq!(*client.bases.last().unwrap(), 0);
    ack(&mut hub, whole[0], false);
    frame(&mut hub);
    client.take();
    assert_eq!(*client.bases.last().unwrap(), whole[0]);
    // (Beside the welcome, only the hotdogs' reserves, `{type:'ammo'}`, which frames do not carry.)
    let words: Vec<Value> = client.inbox.lock().unwrap().iter().filter_map(|m| serde_json::from_str::<Value>(m).ok()).collect();
    assert!(words.iter().filter(|m| m["type"] != "ammo").count() == 1, "nothing but the welcome as text: {words:?}");
}
/// A client plays through the hub over a link that delays each message by 30 to 70 ms each way, in order (as a
/// WebSocket over a jittery network does): it makes a command a step, predicts its hotdog with kitchen-sim's own step,
/// and checks each frame's word against its prediction. Walking a square and jumping as it goes, on the flat with
/// nobody near, the prediction is never wrong by more than a hair.
#[test]
fn a_client_over_a_jittery_link_predicts_its_walk_and_its_jumps_as_the_server_has_them() {
    use crate::hub::{Event, Hub};
    use kitchen_sim::actor::{Actor, World};
    use kitchen_sim::net::{Batch, Mine, decode_frame, peek_frame, pace};
    let t = Table::timed(0., 0.);
    let mut hub = Hub::new(t.lobby, crate::config::Net::default());
    let (outbox, _inbox, frames) = Outbox::memories();
    hub.event(Event::Open { peer: 1, page: false, outbox });
    hub.event(Event::Message { peer: 1, message: json!({"type": "join", "version": 12, "name": "Nell"}) });
    hub.event(Event::Message { peer: 1, message: json!({"type": "ready", "ready": true}) });
    while hub.lobby.game.stage() != "Warmup" {
        hub.advance();
    }
    // Nobody else moves, the wave never comes, and this hotdog stands in the open.
    let me = 1u32;
    for (i, q) in hub.lobby.game.feast.players.iter_mut().enumerate() {
        (q.bot, q.p) = (false, [-1.6 + (i as f32 + 1.) * 0.1, FLOOR, 3.55]);
    }
    hub.lobby.game.feast.warmup = f32::INFINITY;
    hub.lobby.game.feast.players[0].p = [0., FLOOR, 2.6];
    for _ in 0..(1.5 * HZ) as usize {
        hub.advance();
    }
    hub.send();
    let mut client = Feast::with_map(read_map("assets/maps/counter-siege.json"), false);
    for _ in 0..(3. * HZ) as usize {
        client.props.step(&client.level, STEP as f64);
    }
    let mut ghosts = World::default();
    let mut states: Vec<(u32, kitchen_sim::net::State)> = vec![];
    let mut pending: std::collections::VecDeque<(Command, Actor)> = Default::default();
    let (mut up, mut down): (std::collections::VecDeque<(u32, Vec<u8>)>, std::collections::VecDeque<(u32, Vec<u8>)>) = Default::default();
    // (The frames sent before the hotdog was put in the open are not the client's business.)
    let (mut read, mut seq, mut ack, mut wrong, mut worst, mut checked) = (frames.lock().unwrap().len() - 1, 0u32, 0u32, 0, 0f64, 0);
    let mut seed = 12345u32;
    let delay = |seed: &mut u32| {
        *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        2 + (*seed >> 16) % 3
    };
    let (mut due_up, mut due_down) = (0u32, 0u32);
    for step in 0..(12. * HZ) as u32 {
        // The server's step, and its frame if one is due.
        while up.front().is_some_and(|(due, _)| *due <= step) {
            let (_, bytes) = up.pop_front().unwrap();
            hub.event(Event::Binary { peer: 1, bytes });
        }
        if hub.advance() {
            hub.send();
        }
        let sent: Vec<Vec<u8>> = frames.lock().unwrap()[read..].to_vec();
        read += sent.len();
        for bytes in sent {
            due_down = due_down.max(step + delay(&mut seed));
            down.push_back((due_down, bytes));
        }
        // The client: the frames that have come, checked against its prediction.
        while down.front().is_some_and(|(due, _)| *due <= step) {
            let (_, bytes) = down.pop_front().unwrap();
            let (tick, base) = peek_frame(&bytes).unwrap();
            let state = states.iter().find(|(t, _)| *t == base).map(|(_, s)| s);
            let (header, state, _) = decode_frame(&bytes, if base == 0 { None } else { state }).unwrap();
            states.push((tick, state));
            ack = tick;
            let Some(mine) = header.mine else { continue };
            if ghosts.get(me).is_none() {
                let mut body = Actor { radius: kitchen_sim::feast::HOTDOG_RADIUS, ..Actor::hotdog(me, [0.; 3]) };
                mine.onto(&mut body);
                ghosts.add(body);
                pending.clear();
                continue;
            }
            while pending.front().is_some_and(|(c, _)| c.seq < header.ack) {
                pending.pop_front();
            }
            if let Some((c, after)) = pending.front().filter(|(c, _)| c.seq == header.ack).cloned() {
                checked += 1;
                let far = Vec3::from_array(mine.p).as_dvec3().distance(glam::DVec3::from_array(after.p));
                worst = worst.max(far);
                if far > 0.0015 || after.grounded != mine.grounded {
                    wrong += 1;
                    // Corrected as the game does: from the server's word, the commands since again.
                    let body = ghosts.get_mut(me).unwrap();
                    mine.onto(body);
                    let again: Vec<Command> = pending.iter().skip(1).map(|(c, _)| *c).collect();
                    pending.retain(|(k, _)| k.seq > c.seq);
                    let mut redone = vec![];
                    client.rehearse(me, |client| {
                        for c in &again {
                            let jump = if c.has(button::JUMP) { kitchen_sim::actor::JUMP } else { 0. };
                            client.predict(&mut ghosts, me, (false, false), c.stride(pace(false, false, false, 1.), STEP), jump, STEP as f64, false);
                            redone.push(ghosts.get(me).unwrap().clone());
                        }
                    });
                    for ((_, after), body) in pending.iter_mut().zip(redone) {
                        *after = body;
                    }
                    let _ = Mine::of(ghosts.get(me).unwrap());
                }
            }
        }
        // A command a step: a square, a jump every half second.
        if ghosts.get(me).is_some() {
            seq += 1;
            let side = (step / 60) % 4;
            let yaw = side as f32 * std::f32::consts::FRAC_PI_2;
            let c = Command::new(seq, Vec2::new(0., -1.), yaw, 0., if step % 30 == 0 { button::JUMP } else { 0 }, 1, None);
            let jump = if c.has(button::JUMP) { kitchen_sim::actor::JUMP } else { 0. };
            client.predict(&mut ghosts, me, (false, false), c.stride(pace(false, false, false, 1.), STEP), jump, STEP as f64, true);
            pending.push_back((c, ghosts.get(me).unwrap().clone()));
            let batch = Batch { ack, resync: false, time: step, commands: vec![c] };
            due_up = due_up.max(step + delay(&mut seed));
            up.push_back((due_up, batch.encode()));
        }
    }
    assert!(seq > 600 && checked > 300, "it played: {seq} commands, {checked} frames checked");
    assert!(wrong == 0 && worst < 1e-5, "{wrong} frames disagreed with the prediction (at worst by {worst} m)");
}

// ----- Free For All and the dev sandbox (kitchen_sim::rules).

/// A hub over a lobby of `settings`, and a game socket that joins it with `join` (as JSON frames), whose text
/// messages can be read.
fn hub_with(settings: Settings) -> crate::hub::Hub {
    let t = Table::new(settings, None, |_| {});
    crate::hub::Hub::new(t.lobby, crate::config::Net::default())
}
fn socket(hub: &mut crate::hub::Hub, peer: u64, join: Value) -> Inbox {
    use crate::hub::Event;
    let (outbox, inbox, _frames) = Outbox::memories();
    hub.event(Event::Open { peer, page: false, outbox });
    hub.event(Event::Message { peer, message: join });
    inbox
}
fn texts(inbox: &Inbox, kind: &str) -> Vec<Value> {
    inbox.lock().unwrap().iter().filter_map(|m| serde_json::from_str::<Value>(m).ok()).filter(|m| m["type"] == kind).collect()
}
#[test]
fn on_a_dev_server_the_first_join_that_asks_for_the_sandbox_gets_it_later_joins_play_it_and_it_ends_with_the_last_player() {
    use crate::hub::Event;
    let mut hub = hub_with(Settings { dev: true, ..Settings::default() });
    let asked = json!({"sandbox": "", "weapons": "mustard,sniper", "ammo": "inf", "bots": "2", "botmode": "idle", "mode": "forts"});
    let first = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann", "json": true, "sandbox": asked}));
    let welcome = &texts(&first, "welcome")[0];
    assert!(welcome["map"].as_str().unwrap().contains("\"name\": \"Countertop Fort\""), "the mode's default map");
    let game = &hub.lobby.game;
    assert_eq!((game.map.name.as_str(), game.phase, game.stage()), ("Countertop Fort", Phase::Round, "Fight"), "no lobby, no build phase");
    assert!(game.rules().sandbox && game.rules().ammo && game.rules().bots == Some(2));
    let me = game.player(1).unwrap();
    assert!(me.alive && !me.bot && !game.waiting(1) && me.owned[1] && me.owned[2] && !me.owned[6]);
    assert!(!game.feast.fort.as_ref().unwrap().up, "the forcefield is down");
    // The frame says so.
    let frame = texts(&first, "snapshot").pop().unwrap();
    assert_eq!(frame["rules"], json!("sandbox weapons=mustard,sniper ammo=inf bots=2 botmode=idle"));
    assert_eq!(frame["stage"], json!("Fight"));
    // Somebody else joining plays the same game at once, whatever they asked for.
    let second = socket(&mut hub, 2, json!({"type": "join", "version": 12, "name": "Ben", "sandbox": {"sandbox": true, "god": true, "mode": "siege"}}));
    assert_eq!(texts(&second, "welcome").len(), 1);
    let game = &hub.lobby.game;
    assert_eq!((game.map.name.as_str(), game.rules().god), ("Countertop Fort", false));
    assert!(game.player(2).unwrap().alive && !game.waiting(2) && !game.player(2).unwrap().bot);
    for _ in 0..600 {
        hub.advance();
    }
    assert_eq!(hub.lobby.game.stage(), "Fight", "ten seconds on: no phase change");
    assert_eq!(hub.status()["rules"], json!("sandbox weapons=mustard,sniper ammo=inf bots=2 botmode=idle"));
    // The last to leave ends it: the lobby's own game is back.
    hub.event(Event::Closed { peer: 1 });
    assert!(hub.lobby.game.rules().sandbox, "one still plays");
    hub.event(Event::Closed { peer: 2 });
    let game = &hub.lobby.game;
    assert_eq!((game.map.name.as_str(), game.stage(), game.rules().active()), ("Counter Siege", "Lobby", false));
    // And the next sandbox can be another.
    let third = socket(&mut hub, 3, json!({"type": "join", "version": 12, "name": "Cy", "sandbox": {"sandbox": "", "mode": "ffa", "weapons": "all"}}));
    assert_eq!(texts(&third, "welcome").len(), 1);
    let game = &hub.lobby.game;
    assert_eq!((game.map.name.as_str(), game.stage(), game.rules().ffa, game.rules().sandbox), ("Counter Siege", "FreeForAll", true, true));
    assert!((1..kitchen_sim::feast::GUNS).all(|w| game.player(1).unwrap().owned[w]));
}
#[test]
fn without_dev_a_joins_sandbox_is_passed_over_and_a_join_without_one_is_as_it_always_was() {
    let mut hub = hub_with(Settings::default());
    let inbox = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann", "json": true, "sandbox": {"sandbox": "", "god": true}}));
    assert_eq!(texts(&inbox, "welcome").len(), 1);
    assert_eq!((hub.lobby.game.stage(), hub.lobby.game.rules().active()), ("Lobby", false));
    assert_eq!(texts(&inbox, "snapshot").pop().unwrap().get("rules"), None, "no rules in the frame");
    assert_eq!(hub.status()["dev"], json!(false));
    // On a --dev server, a join without `sandbox` is a lobby game as before.
    let mut hub = hub_with(Settings { dev: true, ..Settings::default() });
    let inbox = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann"}));
    assert_eq!(texts(&inbox, "welcome").len(), 1);
    assert_eq!((hub.lobby.game.stage(), hub.lobby.game.rules().active()), ("Lobby", false));
    assert!(texts(&inbox, "error").is_empty());
    // A sandbox asked for while somebody plays is passed over.
    let late = socket(&mut hub, 2, json!({"type": "join", "version": 12, "name": "Ben", "sandbox": {"sandbox": ""}}));
    assert_eq!(texts(&late, "welcome").len(), 1);
    assert!(!hub.lobby.game.rules().active());
}
#[test]
fn free_for_all_is_a_mode_the_leader_chooses_it_plays_any_map_newcomers_sit_down_at_once_and_the_round_ends_in_the_lobby() {
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |_| {});
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(t.lobby.choose(a.member(), Some(&json!("ffa")), None));
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("ffa", "counter-siege"));
    assert!(a.says("Ann chose Free For All on Counter Siege (kitchen): ready up again"));
    // A Forts map, chosen while Free For All is: played as Free For All.
    assert!(t.lobby.choose(a.member(), None, Some(&json!("pillow-fort"))));
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("ffa", "pillow-fort"));
    t.run(0.1);
    let roster = a.last("roster").unwrap();
    assert_eq!((roster["mode"].clone(), roster["choice"]["mode"].clone()), (json!("ffa"), json!("ffa")));
    assert!(roster.get("teams").is_none());
    t.ready(&a, true);
    t.run(0.7);
    let game = &t.lobby.game;
    assert_eq!((game.map.name.as_str(), game.stage(), game.seats(), game.forts(), game.rules().ffa), ("Countertop Fort", "FreeForAll", 5, false, true));
    assert!(snapshot(&mut t.lobby)["ends"].as_f64().unwrap() > 290.);
    // Somebody arriving during the round plays at once.
    let b = t.page(json!({"name": "Ben"}));
    assert!(!t.lobby.game.waiting(b.player.unwrap()) && t.hotdog(&b).alive && !t.hotdog(&b).bot);
    // A death is back FFA_RESPAWN (five) seconds later; FFA_KILLS (25) kills end the round, and everyone is back in the
    // fridge.
    let ann = t.lobby.player(a.member()).unwrap();
    t.lobby.game.feast.hurt(ann, 500.);
    t.run(kitchen_sim::rules::FFA_RESPAWN as f64 + 0.2);
    assert!(t.hotdog(&a).alive && t.hotdog(&a).deaths == 1);
    t.lobby.game.feast.player_mut(ann).unwrap().kills = kitchen_sim::rules::FFA_KILLS;
    t.run(0.1);
    assert_eq!(t.lobby.game.stage(), "RoundOver");
    assert!(t.lobby.game.feast.alert.contains(&format!("Ann WINS WITH {} KILLS", kitchen_sim::rules::FFA_KILLS)), "{}", t.lobby.game.feast.alert);
    t.run(8.4);
    assert_eq!(t.lobby.game.stage(), "Lobby");
    // Random Free For All draws any map.
    assert!(t.lobby.choose(a.member(), Some(&json!("ffa")), Some(&json!("random"))));
    assert!(t.lobby.choice.random && t.lobby.choice.mode == "ffa");
}
#[test]
fn the_sandbox_takes_a_tune_from_a_game_socket_tells_every_player_and_the_rounds_fly_by_it_and_nothing_else_does() {
    use crate::hub::Event;
    let settings = Settings { dev: true, tune: "mustard.speed=1.1".into(), ..Settings::default() };
    let mut hub = hub_with(settings.clone());
    let a = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann", "sandbox": {"sandbox": "", "botmode": "idle"}}));
    assert_eq!(texts(&a, "tuning")[0], json!({"type": "tuning", "text": "mustard.speed=1.1", "editable": true}), "the server's --tune, and that it may change");
    let b = socket(&mut hub, 2, json!({"type": "join", "version": 12, "name": "Ben"}));
    hub.event(Event::Message { peer: 1, message: json!({"type": "tune", "text": "sniper.speed=12 nonsense=2"}) });
    assert_eq!(hub.lobby.game.feast.tune.speed[2], 12.);
    for inbox in [&a, &b] {
        assert_eq!(texts(inbox, "tuning").last().unwrap()["text"], json!("mustard.speed=1.1 sniper.speed=12"));
    }
    // A round of mustard leaves at the tuned speed; the next round keeps the tuning.
    assert!(hub.lobby.game.feast.fire(1, Vec3::NEG_Z, 1));
    let v = Vec3::from_array(hub.lobby.game.feast.projectiles.last().unwrap().v).length();
    assert!((v - 1.1).abs() < 1e-4, "{v}");
    hub.lobby.game.feast.reset();
    assert_eq!(hub.lobby.game.feast.tune.speed[2], 12.);
    hub.event(Event::Message { peer: 2, message: json!({"type": "tune", "text": "reset"}) });
    assert_eq!(texts(&a, "tuning").last().unwrap()["text"], json!(""));
    // The sandbox over, the server's own tuning is back.
    hub.event(Event::Closed { peer: 1 });
    hub.event(Event::Closed { peer: 2 });
    assert_eq!(hub.lobby.game.feast.tune.text(), "mustard.speed=1.1");
    // Outside the sandbox (or without --dev) a tune changes nothing: the sender is told the tuning as it stands.
    let c = socket(&mut hub, 3, json!({"type": "join", "version": 12, "name": "Cy"}));
    hub.event(Event::Message { peer: 3, message: json!({"type": "tune", "text": "mustard.speed=5"}) });
    assert_eq!(hub.lobby.game.feast.tune.speed[1], 1.1);
    assert_eq!(texts(&c, "tuning").last().unwrap(), &json!({"type": "tuning", "text": "mustard.speed=1.1", "editable": false}));
}
#[test]
fn the_command_line_takes_dev_and_a_tuning_and_refuses_a_key_it_does_not_know() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let config = crate::config::parse(&args(&["--dev", "--tune", "mustard.speed=1.1,sniper.speed=12"])).unwrap();
    assert!(config.settings.dev);
    assert_eq!(config.settings.tune, "mustard.speed=1.1 sniper.speed=12");
    assert!(!crate::config::parse(&args(&[])).unwrap().settings.dev);
    let refused = crate::config::parse(&args(&["--tune", "mustard.colour=red"])).unwrap_err();
    assert!(refused.contains("mustard.colour=red"), "{refused}");
}
#[test]
fn bots_skill_on_the_command_line_is_every_lobby_games_unless_a_sandbox_says_otherwise() {
    use kitchen_sim::aim::Skill;
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(crate::config::parse(&args(&[])).unwrap().settings.skill, Skill::Normal);
    let settings = crate::config::parse(&args(&["--bots", "hard"])).unwrap().settings;
    assert_eq!(settings.skill, Skill::Hard);
    for wrong in [&["--bots", "perfect"][..], &["--bots", "tough"], &["--bots"]] {
        assert!(crate::config::parse(&args(wrong)).unwrap_err().contains("--bots"), "{wrong:?}");
    }
    // A lobby game (its rules say nothing of skill) is played at the server's; a sandbox's own skill stands.
    let map = read_map("assets/maps/counter-siege.json");
    assert_eq!(Game::with_rules(map.clone(), &settings, Rules { ffa: true, finite_ammo: true, ..Default::default() }).rules().skill, Skill::Hard);
    assert_eq!(Game::with_rules(map, &settings, Rules::parse("sandbox skill=easy")).rules().skill, Skill::Easy);
}

// ----- Fully Loaded (kitchen_sim::cards): the mode of the lobby, the card table's commands and what sockets are told.

fn cards_of(game: &Game) -> &cards::Table {
    game.feast.cards.as_ref().expect("a card table")
}
/// The shuffle the tests that wait for a person's go are dealt, so that they do not depend on the luck of the cards (a
/// game is a different one each time otherwise, and the hand a person is knocked out of, or a game one hand long, is
/// rare but real). It is the seed a round is dealt from (`Feast::card_seed`), set before the round begins.
const SEED: u64 = 1;
/// The hotdog a person picks for an action card, when they are asked: another one still in the hand if there is one.
fn pick(view: &cards::View, chooser: usize) -> usize {
    (0..view.seats.len()).find(|s| *s != chooser && view.seats[*s].status == cards::Status::In).unwrap_or(chooser)
}
/// A lobby on Fully Loaded with `names` seated (each with a page), the leader having chosen it, everyone ready, and the
/// round begun (the start sequence over).
fn cards_lobby(names: &[&str]) -> (Table, Vec<Page>) {
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |_| {});
    let pages: Vec<Page> = names.iter().map(|name| t.page(json!({"name": name}))).collect();
    t.run(0.1);
    assert!(t.lobby.choose(pages[0].member(), Some(&json!("cards")), None));
    for page in &pages {
        assert!(t.ready(page, true));
    }
    for _ in 0..30 {
        if t.lobby.game.phase == Phase::Round {
            break;
        }
        t.run(0.1);
    }
    assert_eq!(t.lobby.game.phase, Phase::Round);
    (t, pages)
}
/// The seat of the hotdog a page plays: its place among the players, which is how the table knows it.
fn seat_of(t: &Table, page: &Page) -> usize {
    t.lobby.game.seat(t.lobby.player(page.member()).unwrap()).unwrap()
}
/// `favourite` is put a point short of the target, and the game runs until somebody has won it and the lobby has
/// everyone back in the fridge (ten simulated minutes at most: a hand takes under a minute). Returns what was
/// announced when the game was won.
fn play_out(t: &mut Table, favourite: usize) -> String {
    t.lobby.game.feast.cards.as_mut().expect("a card table").set_total(favourite, cards::TARGET - 1);
    let mut announced = None;
    for _ in 0..1200 {
        if t.lobby.game.stage() == "Lobby" {
            break;
        }
        t.run(0.5);
        if t.lobby.game.stage() == "RoundOver" && announced.is_none() {
            announced = Some(t.lobby.game.feast.alert.clone());
        }
    }
    assert_eq!(t.lobby.game.stage(), "Lobby", "nobody won in ten minutes");
    announced.expect("the game was won")
}
#[test]
fn the_leader_chooses_fully_loaded_and_it_plays_on_counter_siege() {
    assert_eq!((lobby::CARDS, lobby::mode_name(lobby::CARDS), lobby::default_map(lobby::CARDS)), ("cards", "Fully Loaded", "counter-siege"));
    assert!(lobby::PLAYS.contains(&lobby::CARDS));
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |_| {});
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("cards", "counter-siege"));
    assert!(a.says("Ann chose Fully Loaded on Counter Siege (kitchen): ready up again"));
    t.run(0.1);
    let roster = a.last("roster").unwrap();
    assert_eq!((roster["mode"].clone(), roster["choice"]["mode"].clone(), roster["choice"]["name"].clone()), (json!("cards"), json!("cards"), json!("counter-siege")));
    assert!(roster.get("teams").is_none(), "no teams at a card table");
    // Counter Siege (and Random, which is it) is the table's place; no other map is, asked for as Fully Loaded.
    for map in ["pillow-fort", "horde-night", "no-such-map"] {
        let notice_before = a.all("notice").len();
        assert!(!t.lobby.choose(a.member(), Some(&json!("cards")), Some(&json!(map))), "{map}");
        assert_eq!(a.all("notice").len(), notice_before + 1);
        assert_eq!(notice(&a), "Fully Loaded is played on Counter Siege.");
        assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("cards", "counter-siege"));
    }
    t.handle(&a, json!({"type": "choose", "mode": "cards", "map": "pillow-fort"}));
    assert_eq!(notice(&a), "Fully Loaded is played on Counter Siege.");
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("cards", "counter-siege"));
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("counter-siege"))), "already chosen");
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("random"))), "Random Fully Loaded is the one table");
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str(), t.lobby.choice.random), ("cards", "counter-siege", false));
    // The fridge opens on a game of Fully Loaded: a table on Counter Siege, and the frame says what it is played by.
    t.ready(&a, true);
    t.run(0.7);
    let game = &t.lobby.game;
    assert_eq!((game.map.name.as_str(), game.rules().cards, game.feast.cards.is_some()), ("Counter Siege", true, true));
    assert_eq!(game.seats(), 5);
    assert_eq!(snapshot(&mut t.lobby)["rules"], json!("mode=cards"));
    assert_eq!(t.lobby.current.mode, "cards");
    t.run(0.1);
    assert_eq!(a.last("roster").unwrap()["playing"]["mode"], json!("cards"));
    // Back in the lobby it can be chosen again, and a mode that is not asked for is a siege as before.
    let favourite = (0..5).find(|s| t.lobby.game.feast.players[*s].bot).unwrap();
    play_out(&mut t, favourite);
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), None));
    assert_eq!(t.lobby.choice.mode, "forts");
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("cards", "counter-siege"));
}
/// The server's console (the lines the hub and `start` print) names the mode a game is played as, not the map's own:
/// Fully Loaded for a card table on Counter Siege, Free For All, and a siege on a Forts map.
#[test]
fn the_console_names_fully_loaded_where_it_names_a_mode() {
    let settings = Settings::default();
    let game = |map: &str, rules: Rules| Game::with_rules(read_map(map), &settings, rules);
    assert_eq!(lobby::played(&game("assets/maps/counter-siege.json", Rules::default())), "Counter Siege (siege)");
    assert_eq!(lobby::played(&game("assets/maps/pillow-fort.json", Rules::default())), "Countertop Fort (forts)");
    assert_eq!(lobby::played(&game("assets/maps/counter-siege.json", Rules { ffa: true, ..Default::default() })), "Counter Siege (ffa)");
    assert_eq!(lobby::played(&game("assets/maps/pillow-fort.json", Rules::parse("mode=siege"))), "Countertop Fort (siege)");
    let cards = game("assets/maps/counter-siege.json", Rules { cards: true, ..Default::default() });
    assert_eq!(lobby::played(&cards), "Counter Siege (Fully Loaded)");
    // The lobby's "next: ..." when the server starts: the choice as it is, and for Fully Loaded, named so.
    let started = Lobby::new(cards, settings.clone(), Options::new(maps_folder()));
    assert_eq!(started.next_up(), "counter-siege (Fully Loaded)");
    let mut t = Table::plain();
    assert_eq!(t.lobby.next_up(), "counter-siege");
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), Some(&json!("random"))));
    assert_eq!(t.lobby.next_up(), "Random (forts)");
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert_eq!(t.lobby.next_up(), "counter-siege (Fully Loaded)");
}
/// A map chosen alone (a link to it, the page's address) while Fully Loaded is chosen is played in its own mode, as with
/// Siege or Forts chosen: only Counter Siege and Random stay Fully Loaded, and Free For All still plays any map. The
/// refusal is for Fully Loaded asked for with another map (`the_leader_chooses_fully_loaded_and_it_plays_on_counter_siege`).
#[test]
fn a_map_chosen_alone_while_fully_loaded_is_chosen_is_played_as_its_own() {
    let mut t = Table::new(Settings::default(), None, |_| {});
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    let choice = |t: &Table| (t.lobby.choice.mode.clone(), t.lobby.choice.name.clone(), t.lobby.choice.random);
    let say = |mode: &str, map: &str| (mode.to_string(), map.to_string(), false);
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert_eq!(choice(&t), say("cards", "counter-siege"));
    let notices = a.all("notice").len();
    // Another map, no mode: its own (a Forts map is Forts, a siege map Siege), and nothing is refused.
    assert!(t.lobby.choose(a.member(), None, Some(&json!("pillow-fort"))));
    assert_eq!(choice(&t), say("forts", "pillow-fort"));
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert!(t.lobby.choose(a.member(), None, Some(&json!("horde-night"))));
    assert_eq!(choice(&t), say("siege", "horde-night"));
    t.handle(&a, json!({"type": "choose", "mode": "cards"}));
    t.handle(&a, json!({"type": "choose", "map": "moonlit-fort"}));
    assert_eq!(choice(&t), say("forts", "moonlit-fort"), "from the page's socket too");
    assert_eq!(a.all("notice").len(), notices, "no refusal along the way");
    // Counter Siege, or Random, chosen while Fully Loaded is chosen, stays Fully Loaded: it is the table's map.
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert!(t.lobby.choose(a.member(), Some(&json!("siege")), Some(&json!("horde-night"))));
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("counter-siege"))), "already Fully Loaded on it");
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("random"))), "and Random is that too");
    assert_eq!(choice(&t), say("cards", "counter-siege"));
    assert!(t.lobby.choose(a.member(), Some(&json!("forts")), Some(&json!("pillow-fort"))));
    assert!(t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("random"))));
    assert_eq!(choice(&t), say("cards", "counter-siege"), "Random while Fully Loaded: the one table");
    // A map that is not there is that, not a refusal to play it as Fully Loaded.
    assert!(!t.lobby.choose(a.member(), None, Some(&json!("no-such-map"))));
    assert_eq!(notice(&a), "There is no map called no-such-map.");
    assert_eq!(choice(&t), say("cards", "counter-siege"));
    // Free For All still plays any map chosen alone, and Random of them.
    assert!(t.lobby.choose(a.member(), Some(&json!("siege")), None));
    assert!(t.lobby.choose(a.member(), Some(&json!("ffa")), None));
    assert!(t.lobby.choose(a.member(), None, Some(&json!("pillow-fort"))));
    assert_eq!(choice(&t), say("ffa", "pillow-fort"), "Free For All plays any map");
    assert!(t.lobby.choose(a.member(), None, Some(&json!("horde-night"))));
    assert_eq!(choice(&t), say("ffa", "horde-night"));
    assert!(t.lobby.choose(a.member(), None, Some(&json!("random"))));
    assert_eq!(choice(&t), ("ffa".to_string(), "random".to_string(), true), "and Random of it");
}
#[test]
fn fully_loaded_needs_counter_siege_in_the_folder() {
    let folder = Folder::new(&["horde-night", "pillow-fort"]);
    let scanner = folder.scanner();
    let mut t = Table::new(Settings::default(), Some(read_map("assets/maps/horde-night.json")), |o| o.maps = scanner);
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(!t.lobby.choose(a.member(), Some(&json!("cards")), None));
    assert_eq!(notice(&a), "There is no map called counter-siege.");
    assert_eq!(t.lobby.choice.name, "horde-night");
}
/// A game of Fully Loaded with one person seated at the first seat and the round begun.
fn cards_round() -> Game {
    let settings = Settings { countdown: 0., opening: 0., ..Settings::default() };
    let mut game = Game::with_rules(read_map("assets/maps/counter-siege.json"), &settings, Rules { cards: true, ..Default::default() });
    game.feast.card_seed = SEED;
    let p = game.join().unwrap();
    assert!(game.ready(p, true));
    game.tick(STEP);
    game.tick(STEP);
    assert_eq!(game.phase, Phase::Round);
    game
}
/// Steps `game` until the table's turn has just come to `seat`, which is the moment it is asked to flip or stop (a
/// person's or a bot's), answering for the person in seat 0 when they are asked to choose a hotdog for an action card.
fn until_the_turn_comes_to(game: &mut Game, seat: usize) {
    for _ in 0..(300. * HZ) as usize {
        let before = cards_of(game).view().turn;
        game.tick(STEP);
        let view = cards_of(game).view();
        if view.turn == Some(seat) && before != Some(seat) {
            return;
        }
        keep_the_game_going(&view, game.feast.cards.as_mut().unwrap());
        if view.choosing.is_some_and(|(chooser, _)| chooser == 0) {
            assert!(game.cards(0, cards::Command::Target(pick(&view, 0))));
        }
    }
    panic!("the turn never came to seat {seat}");
}
#[test]
fn a_person_flips_and_stops_on_their_own_turn_only() {
    let mut game = cards_round();
    assert_eq!(game.seat(1), Some(0));
    // The person's go comes round: they flip, and they have flipped, so the table is not asking them now.
    until_the_turn_comes_to(&mut game, 0);
    assert_eq!(cards_of(&game).view().turn, Some(0));
    // Nobody else's seat is theirs to play: a bot's (every other seat here) and one that is not there.
    let version = cards_of(&game).version();
    for seat in [1, 2, 3, 4, 5, 99, usize::MAX] {
        for command in [cards::Command::Flip, cards::Command::Stop, cards::Command::Target(0), cards::Command::Target(usize::MAX)] {
            assert!(!game.cards(seat, command), "seat {seat}");
        }
    }
    assert_eq!(cards_of(&game).version(), version, "refused: nothing changed");
    let seen = cards_of(&game).view().events.last().map_or(0, |e| e.seq);
    assert!(game.cards(0, cards::Command::Flip), "their own go");
    assert!(cards_of(&game).version() > version);
    let flipped = cards_of(&game).view().events.into_iter().filter(|e| e.seq > seen && e.kind == cards::Kind::Flipped).map(|e| e.seat).collect::<Vec<_>>();
    assert_eq!(flipped, [0], "it was their flip");
    // The card is on show: it is nobody's go now, theirs again included.
    let version = cards_of(&game).version();
    assert!(!game.cards(0, cards::Command::Flip) && !game.cards(0, cards::Command::Stop));
    assert_eq!(cards_of(&game).version(), version);
    // Another go of theirs, in this hand or another: stopping banks the hand, and a hotdog that has stopped is not asked
    // again this hand.
    let mut stopped = false;
    for _ in 0..(300. * HZ) as usize {
        let view = cards_of(&game).view();
        if view.turn == Some(0) && view.clock.is_some() && view.choosing.is_none() {
            assert!(game.cards(0, cards::Command::Stop));
            assert_eq!(cards_of(&game).view().seats[0].status, cards::Status::Stopped);
            assert!(!game.cards(0, cards::Command::Stop) && !game.cards(0, cards::Command::Flip));
            stopped = true;
            break;
        }
        keep_the_game_going(&view, game.feast.cards.as_mut().unwrap());
        if view.choosing.is_some_and(|(chooser, _)| chooser == 0) {
            assert!(game.cards(0, cards::Command::Target(pick(&view, 0))));
        }
        game.tick(STEP);
    }
    assert!(stopped, "their go came round again");
    // Nothing is asked of anybody in the fridge, or before the round: a stray command changes nothing.
    let mut lobby = Game::with_rules(read_map("assets/maps/counter-siege.json"), &Settings::default(), Rules { cards: true, ..Default::default() });
    let p = lobby.join().unwrap();
    let version = cards_of(&lobby).version();
    assert!(!lobby.cards(lobby.seat(p).unwrap(), cards::Command::Flip) && !lobby.cards(lobby.seat(p).unwrap(), cards::Command::Stop));
    assert_eq!(cards_of(&lobby).version(), version);
    // And in a game of anything else there is no table to ask.
    let mut siege = fresh();
    assert!(!siege.cards(0, cards::Command::Flip) && !siege.cards(0, cards::Command::Target(0)));
}
#[test]
fn somebody_who_arrives_during_a_game_waits_in_the_fridge_and_may_not_play_the_bots_seat() {
    let mut game = cards_round();
    let late = game.join().unwrap();
    let seat = game.seat(late).unwrap();
    assert!(game.waiting(late) && game.player(late).unwrap().bot, "the bot plays the seat until the next game");
    // The moment it is the bot's go, the person who is waiting cannot flip, stop or choose for it.
    let mut asked = false;
    for _ in 0..(300. * HZ) as usize {
        let before = cards_of(&game).view().turn;
        game.tick(STEP);
        if cards_of(&game).view().turn == Some(seat) && before != Some(seat) {
            let version = cards_of(&game).version();
            assert!(!game.cards(seat, cards::Command::Stop) && !game.cards(seat, cards::Command::Flip) && !game.cards(seat, cards::Command::Target(0)));
            assert_eq!(cards_of(&game).version(), version);
            asked = true;
            break;
        }
        let view = cards_of(&game).view();
        keep_the_game_going(&view, game.feast.cards.as_mut().unwrap());
        if view.choosing.is_some_and(|(chooser, _)| chooser == 0) {
            assert!(game.cards(0, cards::Command::Target(pick(&view, 0))));
        }
    }
    assert!(asked, "the waiting hotdog's seat was asked");
}
#[test]
fn walking_at_the_table_goes_nowhere() {
    use crate::hub::Event;
    let (mut hub, _) = cards_hub(1);
    let place = |hub: &Hub| -> Vec<[f32; 3]> { hub.lobby.game.feast.players.iter().map(|p| p.p).collect() };
    let bodies = |hub: &Hub| -> Vec<[f64; 3]> { hub.lobby.game.feast.players.iter().map(|p| hub.lobby.game.feast.body(p.id).expect("a body").p).collect() };
    let (seats, held) = (place(&hub), bodies(&hub));
    let weapon = hub.lobby.game.player(1).unwrap().weapon;
    let everything = button::PRONE | button::CROUCH | button::STIM | button::JUMP | button::FIRE | button::MELEE | button::EAT | button::GRAB | button::THROW;
    // Binary batches of commands (eight at most each): walking, and every button on every other one.
    for first in (1..=40).step_by(8) {
        let commands: Vec<Command> = (first..first + 8).map(|seq| Command::new(seq, Vec2::new(0., -1.), 0.7, 0.3, if seq % 2 == 0 { everything } else { button::CROUCH }, 2, Some(0))).collect();
        hub.event(Event::Binary { peer: 1, bytes: kitchen_sim::net::Batch { ack: 0, resync: false, time: 0, commands }.encode() });
    }
    // The debug encoding's, as JSON.
    for first in (41..=80).step_by(8) {
        let commands: Vec<Value> = (first..first + 8).map(|seq| json!({"seq": seq, "stick": [0, -1], "yaw": 2.0, "pitch": 0.3, "prone": true, "crouch": true, "jump": true, "fire": true, "stim": true, "weapon": 3})).collect();
        hub.event(Event::Message { peer: 1, message: json!({"type": "input", "commands": commands}) });
    }
    for _ in 0..120 {
        hub.advance();
        assert_eq!(place(&hub), seats, "the hotdogs stay at their seats");
    }
    assert_eq!(bodies(&hub), held, "and so do their bodies");
    let me = hub.lobby.game.player(1).unwrap();
    assert!(!me.prone && !me.crouch && me.stim == 0. && me.weapon == weapon, "no stance, stim or gun at a card table");
    assert_eq!(hub.lobby.game.applied(1), 80, "the commands were taken as read all the same");
    assert_eq!(hub.lobby.game.waiting_commands(1), 0);
}
#[test]
fn a_person_who_leaves_is_replaced_by_a_bot_that_keeps_their_score() {
    let (mut t, pages) = cards_lobby(&["Ann", "Ben"]);
    let (ann, ben) = (seat_of(&t, &pages[0]), seat_of(&t, &pages[1]));
    t.run(0.1);
    t.lobby.game.feast.cards.as_mut().unwrap().set_total(ben, 42);
    let view = cards_of(&t.lobby.game).view();
    assert!(!view.seats[ben].bot && !view.seats[ann].bot);
    // Ben's game socket closes (hub.rs `closed`).
    let player = t.lobby.player(pages[1].member()).unwrap();
    t.lobby.game.leave(player);
    t.lobby.unseat(pages[1].member());
    t.run(0.1);
    let view = cards_of(&t.lobby.game).view();
    assert!(view.seats[ben].bot, "a bot plays the seat");
    assert!(!view.seats[ann].bot);
    assert_eq!(view.seats[ben].total, 42, "and keeps the score");
    assert_eq!(t.lobby.game.stage(), "Warmup", "the game goes on for Ann");
}
#[test]
fn after_a_winner_everyone_is_back_in_the_fridge() {
    let (mut t, pages) = cards_lobby(&["Ann", "Ben"]);
    let favourite = (0..5).find(|s| t.lobby.game.feast.players[*s].bot).unwrap();
    let announced = play_out(&mut t, favourite);
    assert!(announced.contains(" IS FULLY LOADED: ") && announced.ends_with(" POINTS"), "{announced}");
    assert_eq!(t.lobby.game.stage(), "Lobby");
    assert!(t.lobby.game.feast.players.iter().all(|p| !p.ready), "nobody is ready");
    assert!(pages[0].says("Round over: back in the fridge") && pages[1].says("Round over: back in the fridge"));
    // The next game is another from nothing: a fresh table, and the lobby can start it.
    let fresh = cards_of(&t.lobby.game).view();
    assert!(fresh.winner.is_none() && fresh.seats.iter().all(|s| s.total == 0 && s.cards.is_empty()), "{fresh:?}");
    for page in &pages {
        assert!(t.ready(page, true));
    }
    for _ in 0..30 {
        t.run(0.1);
    }
    assert_eq!(t.lobby.game.phase, Phase::Round);
    assert!(cards_of(&t.lobby.game).view().seats.iter().all(|s| s.total == 0));
}
#[test]
fn choosing_siege_after_fully_loaded_plays_an_ordinary_siege() {
    let (mut t, pages) = cards_lobby(&["Ann"]);
    assert!(t.lobby.game.rules().cards && t.lobby.game.feast.cards.is_some());
    let favourite = (0..5).find(|s| t.lobby.game.feast.players[*s].bot).unwrap();
    play_out(&mut t, favourite);
    assert!(t.lobby.choose(pages[0].member(), Some(&json!("siege")), None));
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str()), ("siege", "counter-siege"));
    t.run(0.1);
    assert_eq!(pages[0].last("roster").unwrap()["mode"], json!("siege"));
    assert!(t.ready(&pages[0], true));
    for _ in 0..30 {
        t.run(0.1);
    }
    let game = &t.lobby.game;
    assert_eq!(game.phase, Phase::Round);
    assert!(game.feast.cards.is_none() && !game.rules().cards && !game.rules().active(), "no table, no rules");
    assert_eq!(t.lobby.current.mode, "siege");
    assert!(snapshot(&mut t.lobby).get("rules").is_none(), "the frame says nothing of cards");
    // And it is a siege: the person walks, and the cats come.
    let me = t.lobby.player(pages[0].member()).unwrap();
    let was = t.lobby.game.player(me).unwrap().p;
    t.lobby.game.commands(me, &walking(1, 30, Vec2::new(0., -1.), 0., 0, 0));
    t.run(0.6);
    assert_ne!(t.lobby.game.player(me).unwrap().p, was, "the hotdog walks");
    // Fully Loaded again, after the siege.
    let winner = t.lobby.player(pages[0].member());
    t.finish(winner);
    assert!(t.lobby.choose(pages[0].member(), Some(&json!("cards")), None));
    assert!(t.ready(&pages[0], true));
    for _ in 0..30 {
        t.run(0.1);
    }
    assert!(t.lobby.game.feast.cards.is_some() && t.lobby.game.rules().cards);
}

// The card table over sockets.

/// A hub whose lobby starts on Fully Loaded (as `--mode cards` starts it) with `n` game sockets (peers 1 to n, in JSON,
/// each in the seat of its number less one) that have readied up, and the round begun.
fn cards_hub(n: u64) -> (Hub, Vec<Inbox>) {
    use crate::hub::Event;
    let settings = Settings { countdown: 0., opening: 0., ..Settings::default() };
    let mut game = Game::with_rules(read_map("assets/maps/counter-siege.json"), &settings, Rules { cards: true, ..Default::default() });
    game.feast.card_seed = SEED;
    let lobby = Lobby::new(game, settings, Options::new(maps_folder()));
    let mut hub = Hub::new(lobby, crate::config::Net::default());
    let inboxes: Vec<Inbox> = (1..=n).map(|peer| socket(&mut hub, peer, json!({"type": "join", "version": 12, "name": format!("P{peer}"), "json": true}))).collect();
    for peer in 1..=n {
        hub.event(Event::Message { peer, message: json!({"type": "ready", "ready": true}) });
    }
    for _ in 0..600 {
        if hub.lobby.game.phase == Phase::Round {
            break;
        }
        hub.advance();
    }
    assert_eq!(hub.lobby.game.phase, Phase::Round);
    // (On a frame step, when the table has been looked at.)
    while !hub.tick.is_multiple_of(2) {
        hub.advance();
    }
    (hub, inboxes)
}
/// Steps the hub until `found` holds of the table's view (ten simulated minutes at most), choosing the hotdog for an
/// action card for each of the first `people` sockets as they are asked to (`pick`). Nobody is let win the game
/// meanwhile (a seat that gets near the target goes back to nothing), so that the table is always to come to it.
fn at_the_table(hub: &mut Hub, people: u64, found: impl Fn(&cards::View) -> bool) -> cards::View {
    use crate::hub::Event;
    for _ in 0..36000 {
        let view = cards_of(&hub.lobby.game).view();
        if found(&view) {
            return view;
        }
        keep_the_game_going(&view, hub.lobby.game.feast.cards.as_mut().unwrap());
        if let Some((seat, _)) = view.choosing.filter(|(seat, _)| (*seat as u64) < people) {
            hub.event(Event::Message { peer: seat as u64 + 1, message: json!({"type": "cards", "action": "target", "seat": pick(&view, seat)}) });
        }
        hub.advance();
    }
    panic!("the table never came to it");
}
/// No seat is let near the target while a test waits for the table: whoever is has its total put back to nothing.
fn keep_the_game_going(view: &cards::View, table: &mut cards::Table) {
    for (seat, shown) in view.seats.iter().enumerate() {
        if shown.total >= cards::TARGET / 2 {
            table.set_total(seat, 0);
        }
    }
}
/// A person's seat is asked to flip or stop: it is their turn, no card is to be chosen, and the clock is running.
fn asked(seat: usize) -> impl Fn(&cards::View) -> bool {
    move |v| v.turn == Some(seat) && v.choosing.is_none() && v.clock.is_some()
}
/// The table's view as JSON, without its clock (the seconds a person has left run on between the versions that are
/// sent, so that what is sent has the clock as it was).
fn view_of(hub: &Hub) -> Value {
    without_clock(serde_json::to_value(cards_of(&hub.lobby.game).view()).unwrap())
}
fn without_clock(mut table: Value) -> Value {
    table["clock"] = Value::Null;
    table
}
#[test]
fn every_game_socket_is_sent_the_table_when_it_changes() {
    use crate::hub::Event;
    let (mut hub, inboxes) = cards_hub(2);
    let (a, b) = (&inboxes[0], &inboxes[1]);
    // Told as they sat down, and again as the round began: each message is {type:'cards', table: the view}.
    for inbox in [a, b] {
        let told = texts(inbox, "cards");
        assert!(told.len() >= 2, "{}", told.len());
        let last = told.last().unwrap();
        assert_eq!(last.as_object().unwrap().len(), 2);
        assert_eq!(without_clock(last["table"].clone()), view_of(&hub), "what the table shows now");
        assert_eq!(last["table"]["seats"].as_array().unwrap().len(), 5);
        assert!(last["table"]["version"].as_u64().unwrap() > 0, "dealt");
    }
    // The clock running is no change: nothing is sent while a person decides.
    at_the_table(&mut hub, 2, asked(0));
    hub.advance();
    hub.advance();
    let (told_a, told_b) = (texts(a, "cards").len(), texts(b, "cards").len());
    let version = cards_of(&hub.lobby.game).version();
    assert_eq!(texts(a, "cards").last().unwrap()["table"]["version"], json!(version));
    for _ in 0..20 {
        hub.advance();
    }
    assert_eq!((texts(a, "cards").len(), texts(b, "cards").len()), (told_a, told_b));
    // A flip is: both are told, on the next frame step.
    hub.event(Event::Message { peer: 1, message: json!({"type": "cards", "action": "flip"}) });
    assert_ne!(cards_of(&hub.lobby.game).version(), version);
    hub.advance();
    hub.advance();
    for inbox in [a, b] {
        let told = texts(inbox, "cards");
        assert_eq!(told.len(), told_a + 1);
        assert_eq!(without_clock(told.last().unwrap()["table"].clone()), view_of(&hub));
        assert_ne!(told.last().unwrap()["table"]["version"], json!(version));
    }
    assert_eq!(texts(a, "cards").last(), texts(b, "cards").last());
    // Somebody who sits down later is told at once, and then as the rest are.
    let c = socket(&mut hub, 3, json!({"type": "join", "version": 12, "name": "P3", "json": true}));
    assert_eq!(texts(&c, "cards").len(), 1);
    assert_eq!(without_clock(texts(&c, "cards")[0]["table"].clone()), view_of(&hub));
    let version = cards_of(&hub.lobby.game).version();
    at_the_table(&mut hub, 2, |v| v.version != version);
    hub.advance();
    hub.advance();
    assert_eq!(texts(&c, "cards").len(), 2);
    assert_eq!(texts(&c, "cards")[1]["table"]["version"], json!(cards_of(&hub.lobby.game).version()));
    // A page's lobby socket is sent none of it.
    let (outbox, lobby_inbox, _) = Outbox::memories();
    hub.event(Event::Open { peer: 4, page: true, outbox });
    hub.event(Event::Message { peer: 4, message: json!({"type": "hello", "name": "Page"}) });
    for _ in 0..600 {
        hub.advance();
    }
    assert!(texts(&lobby_inbox, "cards").is_empty());
    assert!(!texts(&lobby_inbox, "welcome").is_empty());
    for inbox in [a, b, &c] {
        assert!(texts(inbox, "error").is_empty());
    }
}
#[test]
fn a_game_that_is_not_fully_loaded_sends_no_table() {
    let mut hub = hub_with(Settings { countdown: 0., opening: 0., ..Settings::default() });
    let a = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann", "json": true}));
    hub.event(crate::hub::Event::Message { peer: 1, message: json!({"type": "ready", "ready": true}) });
    for _ in 0..600 {
        hub.advance();
    }
    assert_eq!(hub.lobby.game.phase, Phase::Round);
    assert!(texts(&a, "cards").is_empty());
    assert_eq!(texts(&a, "snapshot").last().unwrap().get("rules"), None);
    // A card command in a siege does nothing at all.
    hub.event(crate::hub::Event::Message { peer: 1, message: json!({"type": "cards", "action": "flip"}) });
    assert!(texts(&a, "error").is_empty());
    assert_eq!(hub.status()["mode"], json!("siege"));
}
#[test]
fn a_socket_commands_only_for_its_own_seat_and_malformed_card_messages_change_nothing() {
    use crate::hub::Event;
    let (mut hub, inboxes) = cards_hub(2);
    let say = |hub: &mut Hub, peer: u64, message: Value| hub.event(Event::Message { peer, message });
    // The seat is the socket's own hotdog's, never the message's: while seat 1's person is asked, the other person's
    // stop that names seat 1 changes nothing.
    let view = at_the_table(&mut hub, 2, asked(1));
    assert_eq!(view.seats[1].status, cards::Status::In);
    let (version, shown) = (cards_of(&hub.lobby.game).version(), view_of(&hub));
    for message in [
        json!({"type": "cards", "action": "stop", "seat": 1}),
        json!({"type": "cards", "action": "flip", "seat": 1}),
        json!({"type": "cards", "action": "stop"}),
        json!({"type": "cards", "action": "flip"}),
        json!({"type": "cards", "action": "target", "seat": 1}),
    ] {
        say(&mut hub, 1, message.clone());
        assert_eq!(cards_of(&hub.lobby.game).version(), version, "{message}");
    }
    assert_eq!(view_of(&hub), shown);
    // Malformed ones, from the person whose go it is: nothing changes, nothing is said, nothing panics.
    for junk in [
        json!({"type": "cards"}),
        json!({"type": "cards", "action": "dance"}),
        json!({"type": "cards", "action": "FLIP"}),
        json!({"type": "cards", "action": " stop"}),
        json!({"type": "cards", "action": 7}),
        json!({"type": "cards", "action": null}),
        json!({"type": "cards", "action": ["flip"]}),
        json!({"type": "cards", "action": {"flip": true}}),
        json!({"type": "cards", "action": "target"}),
        json!({"type": "cards", "action": "target", "seat": 99}),
        json!({"type": "cards", "action": "target", "seat": 5}),
        json!({"type": "cards", "action": "target", "seat": -1}),
        json!({"type": "cards", "action": "target", "seat": 1.5}),
        json!({"type": "cards", "action": "target", "seat": "1"}),
        json!({"type": "cards", "action": "target", "seat": null}),
        json!({"type": "cards", "action": "target", "seat": [1]}),
        json!({"type": "cards", "action": "target", "seat": 1e300}),
        json!({"type": "cards", "action": "target", "seat": 18446744073709551615u64}),
        json!({"type": "cards", "action": "target", "seat": 1}),
        json!({"type": "cards", "action": "target", "seat": 0}),
        json!({"type": "cards", "table": {"turn": 1}}),
        json!({"type": "cards", "seat": 1}),
    ] {
        say(&mut hub, 2, junk.clone());
        say(&mut hub, 1, junk.clone());
        assert_eq!(cards_of(&hub.lobby.game).version(), version, "{junk}");
    }
    assert_eq!(view_of(&hub), shown);
    for inbox in &inboxes {
        assert!(texts(inbox, "error").is_empty());
    }
    assert_eq!(hub.status()["humans"], json!(2), "both sockets are still there");
    // Somebody who has not joined is nobody's seat: passed over, and no error (the handshake's timeout is the same).
    let stranger = {
        let (outbox, inbox, _) = Outbox::memories();
        hub.event(Event::Open { peer: 9, page: false, outbox });
        inbox
    };
    say(&mut hub, 9, json!({"type": "cards", "action": "flip"}));
    say(&mut hub, 9, json!({"type": "cards", "action": "target", "seat": 1}));
    assert_eq!(cards_of(&hub.lobby.game).version(), version);
    assert!(texts(&stranger, "error").is_empty());
    // Then the person whose go it is, as themselves: stopping is taken.
    say(&mut hub, 2, json!({"type": "cards", "action": "stop"}));
    assert_ne!(cards_of(&hub.lobby.game).version(), version);
    assert_eq!(cards_of(&hub.lobby.game).view().seats[1].status, cards::Status::Stopped);
    assert_eq!(cards_of(&hub.lobby.game).view().seats[0].cards, view.seats[0].cards, "the other person's cards were not touched");
}
#[test]
fn the_command_line_starts_the_lobby_on_fully_loaded_with_mode_cards_and_only_on_counter_siege() {
    use std::io::{Read, Write};
    let args = |a: &[&str]| -> Vec<String> { ["--port", "0", "--root", ROOT].iter().chain(a).map(|s| s.to_string()).collect() };
    let config = crate::config::parse(&args(&["--mode", "cards"])).unwrap();
    let cards = |c: &crate::config::Config| c.mode.as_deref() == Some("cards");
    assert!(cards(&config) && !config.random && config.map.name == "Counter Siege");
    assert!(!cards(&crate::config::parse(&args(&[])).unwrap()) && !cards(&crate::config::parse(&args(&["--mode", "forts"])).unwrap()));
    // Random is Counter Siege, which is all there is to draw from; Counter Siege by name and by file are it too.
    let random = crate::config::parse(&args(&["--mode", "cards", "--map", "random"])).unwrap();
    assert!(cards(&random) && !random.random && random.map.name == "Counter Siege");
    assert!(cards(&crate::config::parse(&args(&["--mode", "cards", "--map", "counter-siege"])).unwrap()));
    assert!(cards(&crate::config::parse(&args(&["--mode", "cards", "--map", &format!("{ROOT}/assets/maps/counter-siege.json")])).unwrap()));
    // Any other map is refused, with the reason; so is a mode that is none.
    for map in ["pillow-fort", "horde-night"] {
        let refused = crate::config::parse(&args(&["--mode", "cards", "--map", map])).unwrap_err();
        assert!(refused.contains("Fully Loaded is played on Counter Siege"), "{refused}");
    }
    let refused = crate::config::parse(&args(&["--mode", "ctf"])).unwrap_err();
    assert!(refused.contains("--mode needs siege or forts") && refused.contains("cards"), "{refused}");
    // The server starts on it: the status says so, and so does the lobby's choice.
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let running = runtime.block_on(crate::start(config, false)).unwrap();
    let mut stream = std::net::TcpStream::connect(running.addr).unwrap();
    stream.write_all(b"GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let now: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(json!([now["map"], now["mode"], now["choice"]["name"], now["choice"]["mode"], now["stage"], now["rules"]]), json!(["Counter Siege", "cards", "counter-siege", "cards", "Lobby", "mode=cards"]));
    runtime.shutdown_background();
}
#[test]
fn the_dev_sandbox_can_play_fully_loaded_too_and_it_is_on_counter_siege() {
    use crate::hub::Event;
    // The lobby is on Countertop Fort, a Forts map: the sandbox's table is on Counter Siege all the same.
    let t = Table::new(Settings { dev: true, ..Settings::default() }, Some(read_map("assets/maps/pillow-fort.json")), |_| {});
    let mut hub = Hub::new(t.lobby, crate::config::Net::default());
    let a = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann", "json": true, "sandbox": {"sandbox": "", "mode": "cards"}}));
    assert_eq!(texts(&a, "welcome").len(), 1);
    let game = &hub.lobby.game;
    assert_eq!((game.map.name.as_str(), game.phase, game.rules().sandbox, game.rules().cards, game.feast.cards.is_some()), ("Counter Siege", Phase::Round, true, true, true));
    assert_eq!(hub.lobby.current.mode, "cards");
    assert_eq!(hub.status()["mode"], json!("cards"));
    assert_eq!(texts(&a, "cards").len(), 1, "told at once");
    for _ in 0..120 {
        hub.advance();
    }
    assert!(texts(&a, "cards").len() >= 2, "and as it is dealt");
    assert_eq!(without_clock(texts(&a, "cards").last().unwrap()["table"].clone()), view_of(&hub));
    // The last to leave ends it: the lobby's own game is back, with no table.
    hub.event(Event::Closed { peer: 1 });
    let game = &hub.lobby.game;
    assert_eq!((game.map.name.as_str(), game.feast.cards.is_none(), game.rules().cards), ("Countertop Fort", true, false));
    assert_eq!(hub.status()["mode"], json!("forts"));
    // The next to join a game of Fully Loaded is told its table afresh (a new game's versions count from nothing).
    let b = socket(&mut hub, 2, json!({"type": "join", "version": 12, "name": "Ben", "json": true, "sandbox": {"sandbox": "", "mode": "cards"}}));
    for _ in 0..120 {
        hub.advance();
    }
    assert_eq!(without_clock(texts(&b, "cards").last().unwrap()["table"].clone()), view_of(&hub));
}

// ----- The electric griddle (kitchen_sim::griddle): its state travels as JSON text beside the frames.

/// A dev hub whose first join sets up the sandbox on Countertop Fort (no lobby, no build phase), which has a griddle.
fn griddle_hub() -> (Hub, Inbox) {
    let mut hub = hub_with(Settings { dev: true, ..Settings::default() });
    let asked = json!({"sandbox": "", "bots": "0", "mode": "forts"});
    let a = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann", "json": true, "sandbox": asked}));
    assert_eq!(hub.lobby.game.map.name, "Countertop Fort");
    (hub, a)
}
/// Puts player 1's hotdog where it can use the griddle: 0.15 m in front of the dial, looking at it.
fn at_the_dial(hub: &mut Hub) -> usize {
    let game = &mut hub.lobby.game;
    let block = game.map.blocks.iter().position(|b| b.style == "griddle").unwrap();
    let plate = kitchen_sim::griddle::Plate::of(&game.map.blocks[block]);
    let at = [plate.dial[0] + plate.front[0] * 0.15, FLOOR, plate.dial[2] + plate.front[1] * 0.15];
    let p = &mut game.feast.players[0];
    (p.p, p.yaw) = (at, plate.front[0].atan2(plate.front[1]));
    block
}
fn flips(hub: &Hub) -> u32 {
    hub.lobby.game.feast.griddles[0].flips
}
#[test]
fn a_map_with_a_griddle_tells_every_game_socket_its_state_as_they_sit_down_when_it_flips_and_every_second() {
    use crate::hub::Event;
    let (mut hub, a) = griddle_hub();
    let block = at_the_dial(&mut hub);
    // Told as it sat down: off, at room temperature, nobody's.
    let told = texts(&a, "griddle");
    assert_eq!(told.len(), 1);
    assert_eq!(told[0], json!({"type": "griddle", "griddles": [{"block": block, "on": false, "temp": 21.0, "by": 0}]}));
    // Nothing flipped: a sync every second (60 steps), and no more.
    for _ in 0..180 {
        hub.advance();
    }
    let quiet = texts(&a, "griddle").len();
    assert!((3..=5).contains(&quiet), "a sync a second for three seconds: {quiet}");
    assert!(texts(&a, "griddle").iter().all(|m| m["griddles"][0]["on"] == json!(false) && m["griddles"][0]["temp"] == json!(21.0)));
    // Use at the dial: it flips, and everybody is told at once (not on the next sync).
    let b = socket(&mut hub, 2, json!({"type": "join", "version": 12, "name": "Ben", "json": true}));
    assert_eq!(texts(&b, "griddle").len(), 1, "told as it sat down");
    at_the_dial(&mut hub);
    let before = (texts(&a, "griddle").len(), texts(&b, "griddle").len());
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    assert_eq!(flips(&hub), 1);
    for (inbox, was) in [(&a, before.0), (&b, before.1)] {
        let told = texts(inbox, "griddle");
        assert_eq!(told.len(), was + 1);
        assert_eq!(told.last().unwrap(), &json!({"type": "griddle", "griddles": [{"block": block, "on": true, "temp": 21.0, "by": 1}]}));
    }
    // It heats, and the syncs say how much (about 9 degrees a second); the other socket's are the same.
    for _ in 0..180 {
        hub.advance();
    }
    let told = texts(&a, "griddle");
    let temps: Vec<f64> = told.iter().skip(before.0 + 1).map(|m| m["griddles"][0]["temp"].as_f64().unwrap()).collect();
    assert!((2..=4).contains(&temps.len()), "{temps:?}");
    assert!(temps.windows(2).all(|w| w[1] > w[0] + 7. && w[1] < w[0] + 11.), "about nine degrees a second: {temps:?}");
    assert!(temps.last().unwrap() > &40.);
    assert_eq!(told.last(), texts(&b, "griddle").last());
    assert!(told.iter().skip(before.0).all(|m| m["griddles"][0]["on"] == json!(true) && m["griddles"][0]["by"] == json!(1)));
    // Anyone's use: the other player's hotdog (a seat's own, whatever the message says) switches it off.
    {
        let plate = kitchen_sim::griddle::Plate::of(&hub.lobby.game.map.blocks[block]);
        let p = &mut hub.lobby.game.feast.players[1];
        (p.p, p.yaw) = ([plate.dial[0] + 0.1, FLOOR, plate.dial[2] + 0.1], plate.front[0].atan2(plate.front[1]) + 0.2);
    }
    let was = texts(&b, "griddle").len();
    hub.event(Event::Message { peer: 2, message: json!({"type": "griddle", "seat": 1, "action": "toggle"}) });
    assert_eq!(flips(&hub), 2);
    let last = texts(&b, "griddle").pop().unwrap();
    assert_eq!(texts(&b, "griddle").len(), was + 1);
    assert_eq!((last["griddles"][0]["on"].clone(), last["griddles"][0]["by"].clone()), (json!(false), json!(1)), "off, and still the one that switched it on");
    // A page's lobby socket is sent none of it, and nobody gets an error.
    let (outbox, lobby_inbox, _) = Outbox::memories();
    hub.event(Event::Open { peer: 4, page: true, outbox });
    hub.event(Event::Message { peer: 4, message: json!({"type": "hello", "name": "Page"}) });
    for _ in 0..120 {
        hub.advance();
    }
    assert!(texts(&lobby_inbox, "griddle").is_empty());
    for inbox in [&a, &b] {
        assert!(texts(inbox, "error").is_empty());
    }
}
#[test]
fn use_at_the_griddle_flips_it_only_for_a_hotdog_within_reach_of_the_dial_facing_it_and_in_a_round() {
    use crate::hub::Event;
    let (mut hub, a) = griddle_hub();
    let told = texts(&a, "griddle").len();
    // Out of reach (the hotdog is where it began, in its base), turned away from the dial, and a stranger that has not joined.
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    assert_eq!(flips(&hub), 0);
    at_the_dial(&mut hub);
    hub.lobby.game.feast.players[0].yaw += 3.;
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    assert_eq!(flips(&hub), 0);
    let stranger = {
        let (outbox, inbox, _) = Outbox::memories();
        hub.event(Event::Open { peer: 9, page: false, outbox });
        inbox
    };
    at_the_dial(&mut hub);
    hub.event(Event::Message { peer: 9, message: json!({"type": "griddle"}) });
    assert_eq!(flips(&hub), 0);
    assert!(texts(&stranger, "error").is_empty());
    assert_eq!(texts(&a, "griddle").len(), told, "nothing flipped, nothing sent");
    // Dead: nothing.
    hub.lobby.game.feast.players[0].alive = false;
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    assert_eq!(flips(&hub), 0);
    hub.lobby.game.feast.players[0].alive = true;
    // As itself, within reach, facing the dial: it flips. Pressed twice at once it flips once (the griddle holds still a moment).
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    assert_eq!(flips(&hub), 1);
    assert_eq!(texts(&a, "griddle").len(), told + 1);
    // Not in a round (the fridge): passed over. (A lobby game is a game in the lobby phase.)
    let mut lobby = hub_with(Settings::default());
    let c = socket(&mut lobby, 1, json!({"type": "join", "version": 12, "name": "Cy", "json": true}));
    lobby.lobby.game = Game::new(read_map("assets/maps/pillow-fort.json"), &Settings::default());
    assert_eq!(lobby.lobby.game.phase, Phase::Lobby);
    assert!(!lobby.lobby.game.use_griddle(1));
    assert!(texts(&c, "error").is_empty());
}
#[test]
fn a_new_round_tells_the_sockets_the_griddle_is_off_again_and_a_map_without_one_says_nothing() {
    use crate::hub::Event;
    let (mut hub, a) = griddle_hub();
    at_the_dial(&mut hub);
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    for _ in 0..240 {
        hub.advance();
    }
    assert!(texts(&a, "griddle").last().unwrap()["griddles"][0]["temp"].as_f64().unwrap() > 30.);
    // The round starts afresh (the game does this by itself at a round's end): the griddle is off and cold, and the sockets hear so at the next frame.
    hub.lobby.game.feast.reset();
    assert_eq!((hub.lobby.game.feast.griddles[0].on, hub.lobby.game.feast.griddles[0].temp), (false, 21.));
    let was = texts(&a, "griddle").len();
    for _ in 0..4 {
        hub.advance();
    }
    assert_eq!(texts(&a, "griddle").len(), was + 1);
    let last = texts(&a, "griddle").pop().unwrap();
    assert_eq!((last["griddles"][0]["on"].clone(), last["griddles"][0]["temp"].clone(), last["griddles"][0]["by"].clone()), (json!(false), json!(21.0), json!(0)));
    // A map without a griddle (Counter Siege) has nothing to say, and a use at nothing is nothing.
    let mut hub = hub_with(Settings { countdown: 0., opening: 0., ..Settings::default() });
    let b = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ben", "json": true}));
    hub.event(Event::Message { peer: 1, message: json!({"type": "ready", "ready": true}) });
    for _ in 0..600 {
        hub.advance();
    }
    assert_eq!(hub.lobby.game.phase, Phase::Round);
    hub.event(Event::Message { peer: 1, message: json!({"type": "griddle"}) });
    assert!(texts(&b, "griddle").is_empty() && texts(&b, "error").is_empty());
}

// ----- Kung Fu (kitchen_sim::kungfu): a map mode of its own, its fight told as JSON text beside the frames.

#[test]
fn the_leader_chooses_kung_fu_and_it_plays_on_the_dojo_and_free_for_all_leaves_the_dojo_out() {
    assert_eq!((lobby::KUNGFU, lobby::mode_name(lobby::KUNGFU), lobby::default_map(lobby::KUNGFU)), ("kungfu", "Kung Fu", "kung-fu-dojo"));
    assert!(lobby::PLAYS.contains(&lobby::KUNGFU));
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |_| {});
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(t.lobby.choose(a.member(), Some(&json!("kungfu")), None));
    assert_eq!((t.lobby.choice.mode.as_str(), t.lobby.choice.name.as_str(), t.lobby.mode()), ("kungfu", "kung-fu-dojo", "kungfu"));
    assert!(a.says("Ann chose Kung Fu on Kung Fu Dojo (kitchen): ready up again"), "{:?}", a.all("system"));
    t.run(0.1);
    assert_eq!(a.last("roster").unwrap()["mode"], json!("kungfu"));
    // The fridge opens on the dojo, played as Kung Fu: no guns, no pickups, a fighter for every hotdog.
    t.ready(&a, true);
    for _ in 0..30 {
        if t.lobby.game.phase == Phase::Round {
            break;
        }
        t.run(0.1);
    }
    let game = &t.lobby.game;
    assert_eq!((game.phase, game.map.name.as_str(), game.stage()), (Phase::Round, "Kung Fu Dojo", "FreeForAll"));
    assert!(game.rules().kungfu && game.feast.pickups.is_empty());
    let view = game.feast.kungfu_view().expect("a fight");
    assert_eq!((view.fighters.len(), view.goal), (game.seats(), kitchen_sim::kungfu::KOS));
    assert_eq!(lobby::rules_for(&t.lobby.current), Rules { kungfu: true, ..Default::default() });
    // Free For All is played on the siege and Forts maps, not the dojo.
    let mut t = Table::new(Settings::default(), None, |_| {});
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(t.lobby.choose(a.member(), Some(&json!("ffa")), None));
    assert_ne!(t.lobby.choice.name, "kung-fu-dojo");
}
#[test]
fn the_command_line_starts_the_lobby_on_kung_fu_with_mode_kungfu() {
    let args = |list: &[&str]| list.iter().map(|s| s.to_string()).chain(["--root".into(), ROOT.into()]).collect::<Vec<String>>();
    let config = crate::config::parse(&args(&["--mode", "kungfu"])).unwrap();
    assert_eq!((config.map.name.as_str(), config.map.mode.as_str()), ("Kung Fu Dojo", "kungfu"));
    let game = Game::with_rules(config.map.clone(), &config.settings, Rules::default());
    assert!(game.rules().kungfu && game.feast.kungfu.is_some(), "a kungfu map is Kung Fu whatever the rules");
}
#[test]
fn the_fight_reaches_every_game_socket_as_json_beside_unchanged_frames_and_a_persons_command_strikes() {
    use crate::hub::Event;
    use kitchen_sim::kungfu::{Move, Slot, View};
    use kitchen_sim::net::Batch;
    let settings = Settings { countdown: 0., opening: 0., ..Settings::default() };
    let game = Game::with_rules(read_map("assets/maps/kung-fu-dojo.json"), &settings, Rules::default());
    let lobby = Lobby::new(game, settings, Options::new(maps_folder()));
    let mut hub = Hub::new(lobby, crate::config::Net::default());
    let (outbox, a, frames) = Outbox::memories();
    hub.event(Event::Open { peer: 1, page: false, outbox });
    hub.event(Event::Message { peer: 1, message: json!({"type": "join", "version": 12, "name": "Ann"}) });
    let b = socket(&mut hub, 2, json!({"type": "join", "version": 12, "name": "Bob", "json": true}));
    // Told as they sat down.
    let me = texts(&a, "welcome")[0]["id"].as_u64().unwrap() as u32;
    for inbox in [&a, &b] {
        let told = texts(inbox, "kungfu");
        assert!(!told.is_empty(), "told as it sat down");
        let view: View = serde_json::from_value(told[0].clone()).unwrap();
        assert_eq!(view.fighters.len(), 5);
    }
    for peer in [1, 2] {
        hub.event(Event::Message { peer, message: json!({"type": "ready", "ready": true}) });
    }
    for _ in 0..600 {
        if hub.lobby.game.phase == Phase::Round {
            break;
        }
        hub.advance();
    }
    assert_eq!(hub.lobby.game.phase, Phase::Round);
    // A person presses the trigger: a light strike in its style, which the next message says.
    let style = hub.lobby.game.feast.kungfu.as_ref().unwrap().fighter(me).unwrap().style;
    let want = style.move_in(Slot::Light1);
    let mut found = None;
    for k in 0..240u32 {
        let free = hub.lobby.game.feast.kungfu_view().unwrap().fighter(me).is_some_and(|f| kitchen_sim::kungfu::free(f, hub.lobby.game.feast.elapsed));
        let buttons = if free && k % 2 == 1 { button::FIRE } else { 0 };
        let c = Command::new(k + 1, Vec2::ZERO, 0., 0., buttons, 0, None);
        hub.event(Event::Binary { peer: 1, bytes: Batch { ack: 0, resync: false, time: k, commands: vec![c] }.encode() });
        hub.advance();
        let told: Vec<View> = texts(&b, "kungfu").into_iter().map(|v| serde_json::from_value(v).unwrap()).collect();
        if let Some(v) = told.iter().rev().find(|v| v.fighter(me).is_some_and(|f| f.mv() == want)) {
            found = Some(v.clone());
            break;
        }
    }
    let v = found.expect("the strike was told");
    let f = v.fighter(me).unwrap();
    assert!(f.started <= v.elapsed && (f.len - want.data().total() * f.scale).abs() < 1e-4);
    assert_eq!(Move::from_u8(f.mv), Some(want));
    // Frames are as they always were (binary, protocol 12): they still decode, and the style is the hotdog's weapon.
    let bytes = frames.lock().unwrap().last().cloned().expect("binary frames");
    assert_eq!(bytes[0], kitchen_sim::net::FRAME);
    let player = hub.lobby.game.player(me).unwrap();
    assert_eq!(player.weapon, style as usize + 1);
    // The fight is not re-sent while nothing in it changes: at most one message a frame.
    let before = texts(&b, "kungfu").len();
    for _ in 0..60 {
        hub.advance();
    }
    assert!(texts(&b, "kungfu").len() - before <= 30);
}

// ----- Magazines and reloading (kitchen_sim::magazine).

/// R travels in a command as `button::RELOAD`, both of TIP's bits (a tip no pose has): with its hands free the hotdog
/// reloads the gun in hand, and taking another gun in hand cancels it; carrying something, the bits are how it is held
/// and nothing is reloaded. A command dropped from a full queue keeps its reload, and the debug encoding's `reload` is the
/// same bit.
#[test]
fn r_travels_in_a_command_and_reloads_the_gun_in_hand_and_carrying_the_bits_are_still_a_pose() {
    use kitchen_sim::feast::build::Pose;
    use kitchen_sim::magazine::RELOAD;
    let mut s = fresh();
    s.feast.warmup = f32::INFINITY;
    let p = s.feast.player_mut(1).unwrap();
    (p.ammo[RIFLE], p.reserve[RIFLE], p.weapon, p.gun) = (10, 100, RIFLE, RIFLE);
    let r = |seq: u32, buttons: u16, weapon: usize| Command::new(seq, Vec2::ZERO, 0., 0., buttons, weapon as u8, None);
    assert!(r(1, button::RELOAD, RIFLE).reloads());
    assert!((0..3).all(|tip| !r(1, Command::posed(Pose { turn: 5, tip }), 0).reloads()), "no pose is a reload");
    assert_eq!(r(1, button::RELOAD, 0).pose(), Pose::default(), "and a reload is no pose");
    play(&mut s, 1, &[r(1, button::RELOAD, RIFLE)]);
    assert!(s.feast.reloading(1));
    let steps = (RELOAD[RIFLE] as f64 * HZ).round() as u32 + 2;
    play(&mut s, 1, &(2..2 + steps).map(|k| r(k, 0, RIFLE)).collect::<Vec<_>>());
    let me = s.player(1).unwrap();
    assert_eq!((me.ammo[RIFLE], me.reserve[RIFLE], s.feast.reloading(1)), (60, 50, false));
    // Another gun in hand cancels it, and nothing goes in.
    s.feast.player_mut(1).unwrap().ammo[RIFLE] = 20;
    let k = 2 + steps;
    play(&mut s, 1, &[r(k, button::RELOAD, RIFLE), r(k + 1, 0, RIFLE), r(k + 2, 0, 1)]);
    assert!(!s.feast.reloading(1));
    play(&mut s, 1, &(k + 3..k + 3 + steps).map(|k| r(k, 0, RIFLE)).collect::<Vec<_>>());
    assert_eq!(s.player(1).unwrap().ammo[RIFLE], 20);
    // A reload in a command dropped from a full queue is kept for the next.
    let k = k + 3 + steps;
    let many: Vec<Command> = (0..crate::game::QUEUE as u32 + 1).map(|i| r(k + i, if i == 0 { button::RELOAD } else { 0 }, RIFLE)).collect();
    s.commands(1, &many);
    s.tick(STEP);
    assert!(s.feast.reloading(1), "the dropped command's reload went with the next");
    // The debug encoding's `reload`: the same bit, the pose's bits cleared.
    let c = crate::protocol::commands(&json!({"commands": [{"seq": 1, "reload": true, "turn": 3}, {"seq": 2, "turn": 3, "tip": 2}]}));
    assert!(c[0].reloads() && c[0].buttons & button::TURN == 0 && !c[1].reloads());
}
/// Carrying, both of TIP's bits are not R: a hotdog with a loose object in its hands is not reloaded by them.
#[test]
fn carrying_a_loose_object_the_pose_bits_reload_nothing() {
    let mut s = round_on(read_map("assets/maps/pillow-fort.json"), Settings::default());
    for q in s.feast.players.iter_mut() {
        q.bot = false;
    }
    s.feast.set_build(600.);
    s.feast.players[0].p = [-2.2, FLOOR, 2.64];
    s.tick(STEP);
    play(&mut s, 1, &[Command::new(1, Vec2::ZERO, 0., -0.7, button::GRAB, 0, None), Command::new(2, Vec2::ZERO, 0., -0.7, 0, 0, None)]);
    assert!(s.player(1).unwrap().carry >= 0);
    // (In the fight, where a reload could be had.)
    s.feast.stage = Stage::Fight;
    (s.feast.player_mut(1).unwrap().ammo[RIFLE], s.feast.player_mut(1).unwrap().reserve[RIFLE]) = (10, 100);
    play(&mut s, 1, &[Command::new(3, Vec2::ZERO, 0., 0., button::RELOAD, 0, None)]);
    assert!(!s.feast.reloading(1) && s.player(1).unwrap().carry >= 0);
}
/// Every game socket hears each hotdog's reserve and the reload under way (`{type:'ammo'}`) when it sits down and
/// whenever any of it changes, and not otherwise; what it hears sets its copy of the round as the server has it.
#[test]
fn the_reserves_and_reloads_reach_every_game_socket_when_they_change() {
    use kitchen_sim::magazine::{Stock, RELOAD};
    let mut hub = hub_with(Settings::default());
    let a = socket(&mut hub, 1, json!({"type": "join", "version": 12, "name": "Ann"}));
    let told = texts(&a, "ammo");
    assert_eq!(told.len(), 1, "on sitting down");
    let stocks: Vec<Stock> = serde_json::from_value(told[0]["hotdogs"].clone()).unwrap();
    assert_eq!(stocks.len(), hub.lobby.game.feast.players.len());
    assert_eq!(stocks, hub.lobby.game.feast.stocks());
    // Nothing changes: nothing more is sent.
    for _ in 0..10 {
        hub.advance();
    }
    let before = texts(&a, "ammo").len();
    for _ in 0..10 {
        hub.advance();
    }
    assert_eq!(texts(&a, "ammo").len(), before, "nothing changed");
    // A reserve changes and a reload begins: both are told, once each.
    let id = hub.lobby.game.feast.players[0].id;
    let p = hub.lobby.game.feast.player_mut(id).unwrap();
    (p.alive, p.weapon, p.ammo[RIFLE], p.reserve[RIFLE], p.carry) = (true, RIFLE, 5, 77, -1);
    for _ in 0..4 {
        hub.advance();
    }
    let last: Vec<Stock> = serde_json::from_value(texts(&a, "ammo").last().unwrap()["hotdogs"].clone()).unwrap();
    assert_eq!(last.iter().find(|s| s.id == id).unwrap().reserve[RIFLE], 77);
    let sent = texts(&a, "ammo").len();
    hub.lobby.game.feast.stage = Stage::Wave;
    assert!(hub.lobby.game.feast.reload(id));
    for _ in 0..4 {
        hub.advance();
    }
    let told = texts(&a, "ammo");
    assert_eq!(told.len(), sent + 1, "the reload, once");
    let last: Vec<Stock> = serde_json::from_value(told.last().unwrap()["hotdogs"].clone()).unwrap();
    let mine = last.iter().find(|s| s.id == id).unwrap();
    assert_eq!((mine.gun, mine.of), (RIFLE, RELOAD[RIFLE]));
    // A client's copy of the round takes it in: the reserve and the reload as the server has them.
    // (At the moment the server's reload stands at: in the lobby the round's clock runs on without the round.)
    let mut copy = Feast::new(false);
    let r = hub.lobby.game.feast.player(id).unwrap().reload.unwrap();
    copy.elapsed = r.at + r.of - r.left;
    copy.restock(&last);
    let (theirs, ours) = (copy.player(id).unwrap(), hub.lobby.game.feast.player(id).unwrap());
    assert_eq!(theirs.reserve, ours.reserve);
    assert!((theirs.reload.unwrap().left - ours.reload.unwrap().left).abs() < 1e-3, "{:?} against {:?} at {}", theirs.reload, ours.reload, copy.elapsed);
}
/// Free For All from the lobby plays by its finite rounds: the guns start with spare magazines and are not topped up.
#[test]
fn free_for_all_from_the_lobby_runs_its_rounds_out() {
    use kitchen_sim::magazine::SPARE;
    let mut t = Table::new(Settings { countdown: 0.2, opening: 0.3, ..Settings::default() }, None, |_| {});
    let a = t.page(json!({"name": "Ann"}));
    t.run(0.1);
    assert!(t.lobby.choose(a.member(), Some(&json!("ffa")), None));
    t.ready(&a, true);
    t.run(0.7);
    let rules = t.lobby.game.rules().clone();
    assert!(rules.ffa && rules.finite_ammo && !rules.unlimited(), "{rules:?}");
    assert_eq!(rules.text(), "mode=ffa", "what the clients are told");
    let me = t.hotdog(&a).clone();
    assert_eq!((me.ammo[RIFLE], me.reserve[RIFLE], me.ammo[1], me.reserve[1]), (60, SPARE[RIFLE], 12, SPARE[1]));
    let ann = t.lobby.player(a.member()).unwrap();
    t.lobby.game.feast.player_mut(ann).unwrap().ammo[1] = 0;
    t.run(0.2);
    assert_eq!(t.hotdog(&a).ammo[1], 0, "not topped up");
}

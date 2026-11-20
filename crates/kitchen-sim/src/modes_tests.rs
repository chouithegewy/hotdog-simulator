// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The modes (rules.rs `Mode`) and the basics every gun mode shares: the same loadout, the same pickups and power
//! weapons, the same F and the same score for a kill in the siege's wave and famine, Free For All and Forts' build and
//! fight; the mode a game's rules choose rather than its map; and every gun mode on every map it says it can be played on.
use super::*;
use crate::feast::{chest, kind_of, muzzle, Damage, Player, BACKSTAB, MELEE, RIFLE, SNIPER};
use crate::magazine::{stock, CARRY, COMEBACK, LOADOUT, PICKUP, POWER, POWER_BACK, SPARE};
use crate::map::{AMMO_BOX, BUILD, FORT_PLAYERS};

const DT: f32 = 1. / 30.;

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|error| panic!("{path}: {error}"))
}
fn map(name: &str) -> Map {
    Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap()
}
/// Every map the game ships with, by its file's name.
fn maps() -> Vec<(String, Map)> {
    let dir = format!("{}/../../assets/maps", env!("CARGO_MANIFEST_DIR"));
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().filter_map(|f| f.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string)).collect();
    names.sort();
    names.into_iter().map(|name| (name.clone(), map(&name))).collect()
}
/// A game on `map` as the server sets one out: `mode`'s rules (None: the map's own).
fn game_on(map: Map, mode: Option<Mode>) -> Feast {
    let mut s = Feast::with_map(map, false);
    let rules = mode.map_or_else(Rules::default, |m| Rules::playing(m, &s.level.map));
    s.set_rules(rules);
    s
}
fn game(name: &str, mode: Option<Mode>) -> Feast {
    game_on(map(name), mode)
}
fn run(s: &mut Feast, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        s.tick(DT);
    }
}
/// A Forts game on `name` past its (short) build phase.
fn fight(name: &str, mode: Option<Mode>) -> Feast {
    let mut s = game(name, mode);
    s.set_build(1.);
    run(&mut s, 1.2);
    assert_eq!(s.stage, Stage::Fight);
    s
}
/// Every gun mode at the start of each of its phases, on its own kind of map and on another's: (what it is, the game).
fn phases() -> Vec<(&'static str, Feast)> {
    let mut famine = game("counter-siege", Some(Mode::Siege));
    famine.begin_famine();
    let mut far = game("pillow-fort", Some(Mode::Siege));
    far.begin_famine();
    vec![
        ("the siege's warm-up and wave", game("counter-siege", None)),
        ("the siege's famine", famine),
        ("Free For All", game("counter-siege", Some(Mode::Ffa))),
        ("Forts' build", game("pillow-fort", None)),
        ("Forts' fight", fight("pillow-fort", None)),
        ("a siege on a Forts map", game("pillow-fort", Some(Mode::Siege))),
        ("a siege's famine on a Forts map", far),
        ("Free For All on a Forts map", game("pillow-fort", Some(Mode::Ffa))),
        ("Horde Night", game("horde-night", None)),
    ]
}
/// The loadout of every gun mode, as a hotdog's guns.
fn loaded(p: &Player) -> bool {
    p.owned == [false, true, false, false, false, false, true, false] && (p.weapon, p.gun) == (RIFLE, RIFLE) && p.ammo == [-1, 12, 0, 0, 0, 0, 60, 0] && p.reserve == [0, 48, 0, 0, 0, 0, 180, 0] && p.reload.is_none()
}

#[test]
fn every_gun_mode_starts_everyone_person_and_bot_with_the_rifle_60_and_180_and_the_pistol_12_and_48() {
    assert_eq!(LOADOUT, [RIFLE, 1]);
    assert_eq!((SPARE[RIFLE], SPARE[1], CARRY[RIFLE], CARRY[1]), (180, 48, 180, 48));
    for (what, s) in phases() {
        assert!(s.players.iter().any(|p| !p.bot) && s.players.iter().any(|p| p.bot), "{what}: a person and bots");
        for p in &s.players {
            assert!(loaded(p), "{what}: {} has {:?} {:?} {:?}", p.name, p.owned, p.ammo, p.reserve);
        }
    }
    // A new round, and a hotdog taken over (the server's `fit`), are fitted out the same; so is Free For All's respawn.
    for (what, mut s) in phases() {
        s.reset();
        assert!(s.players.iter().all(loaded), "{what}: the next round");
        s.player_mut(2).unwrap().ammo = [-1, 0, 0, 0, 0, 0, 0, 0];
        s.fit(2);
        assert!(loaded(s.player(2).unwrap()), "{what}: fitted out again");
    }
    let mut s = game("counter-siege", Some(Mode::Ffa));
    for p in &mut s.players {
        p.bot = false;
    }
    s.player_mut(3).unwrap().reserve = [0; crate::feast::GUNS];
    s.hurt(3, 500.);
    run(&mut s, crate::rules::FFA_RESPAWN + 0.1);
    assert!(s.player(3).unwrap().alive && loaded(s.player(3).unwrap()), "back from the dead with it");
    // The sandbox's `ammo=inf` still keeps every gun full, and its own loadout is still a person's.
    let s = game_on(map("counter-siege"), None);
    let mut boxed = s;
    boxed.set_rules(Rules::parse("sandbox ammo=inf weapons=sniper bots=0"));
    let me = boxed.player(1).unwrap();
    assert!(boxed.rules.unlimited() && me.owned[SNIPER] && !me.owned[RIFLE] && me.reserve[SNIPER] == CARRY[SNIPER]);
}

#[test]
fn every_gun_mode_gives_the_same_pickups_whatever_the_map_says_one_magazine_and_the_power_weapons_4_and_2() {
    assert_eq!(PICKUP, [0, 12, 4, 90, 2, 40, 60, 150]);
    assert_eq!((COMEBACK, POWER, POWER_BACK), ([20., 12.], [SNIPER, 4], [30., 60.]));
    // The maps say otherwise here and there (Countertop Fort: a sniper 5, the hot sauce 80, a bazooka 3, a rifle 30;
    // Counter Siege: a sniper 4): the rules' table is what every pickup gives, in every mode.
    let pillow = map("pillow-fort");
    assert!(pillow.pickups.iter().any(|p| p.weapon == SNIPER && p.ammo == 5) && pillow.pickups.iter().any(|p| p.weapon == 6 && p.ammo == 30));
    for (what, s) in phases() {
        assert!(!s.pickups.is_empty());
        for item in &s.pickups {
            assert_eq!((item.ammo, item.respawn), stock(item.weapon), "{what}: pickup {} ({})", item.id, item.weapon);
            if item.weapon != AMMO_BOX {
                assert_eq!(item.ammo, PICKUP[item.weapon]);
            }
        }
    }
    // A map's own `respawn` is read and passed over.
    let mut text: serde_json::Value = serde_json::from_str(&read("assets/maps/counter-siege.json")).unwrap();
    text["respawn"] = serde_json::json!([5, 3]);
    let told = Map::from_value(&text).unwrap();
    assert_eq!(told.respawn, [5., 3.]);
    for mode in Mode::GUNS.into_iter().filter(|m| told.refuses(*m).is_none()) {
        let s = game_on(told.clone(), Some(mode));
        assert!(s.pickups.iter().all(|item| item.respawn == stock(item.weapon).1), "{mode:?}");
    }
}

#[test]
fn the_power_weapons_are_scarce_in_every_gun_mode_a_box_never_tops_them_up_and_they_come_back_30_to_60_seconds_later() {
    // A box tops up every gun a hotdog holds but the sniper and the bazooka: the same rule wherever it is asked.
    let (mut owned, reserve) = ([false; crate::feast::GUNS], [0; crate::feast::GUNS]);
    for w in 1..crate::feast::GUNS {
        owned[w] = true;
    }
    assert_eq!(crate::feast::refills(&owned, &reserve), [1, 3, 5, 6, 7]);
    // Taken in each gun mode, a sniper gives its four and comes back between 30 and 60 seconds later (drawn); another
    // gun 20.
    for (what, mut s) in [("a siege", game("counter-siege", None)), ("the famine", { let mut s = game("counter-siege", None); s.begin_famine(); s }), ("Free For All", game("counter-siege", Some(Mode::Ffa))), ("Forts", fight("pillow-fort", None))] {
        for p in &mut s.players {
            p.bot = false;
        }
        let me = s.players[0].team;
        let side = |s: &Feast, i: usize| s.level.fort.as_ref().is_none_or(|fort| fort.sides[me as usize] == s.level.side(s.pickups[i].p[0] as f64, s.pickups[i].p[2] as f64));
        let sniper = (0..s.pickups.len()).find(|i| s.pickups[*i].weapon == SNIPER && side(&s, *i)).expect("a sniper");
        let other = (0..s.pickups.len()).find(|i| !POWER.contains(&s.pickups[*i].weapon) && s.pickups[*i].weapon != AMMO_BOX).expect("another gun");
        for (item, power) in [(sniper, true), (other, false)] {
            let pad = s.pickups[item].p;
            s.player_mut(1).unwrap().p = [pad[0], pad[1] - 0.04, pad[2]];
            s.embody();
            let w = s.pickups[item].weapon;
            if s.player(1).unwrap().owned[w] {
                s.player_mut(1).unwrap().reserve[w] = 0;
            }
            assert!(s.pickup(1, item), "{what}: pickup {item}");
            let back = s.pickups[item].ready - s.elapsed;
            if power {
                assert!((POWER_BACK[0]..=POWER_BACK[1]).contains(&back), "{what}: {back}");
                assert_eq!(s.player(1).unwrap().ammo[SNIPER], 4, "{what}");
            } else {
                assert_eq!(back, COMEBACK[0], "{what}");
            }
        }
        // And no box tops the sniper up.
        let p = s.player_mut(1).unwrap();
        p.reserve = [0; crate::feast::GUNS];
        assert!(!Feast::short(p).contains(&SNIPER), "{what}");
    }
}

/// Hotdog 1 (a person) 0.14 m behind hotdog 2 (a person, facing away), hotdog 1 aiming at its chest; everybody else
/// far off and nobody a bot.
fn behind(s: &mut Feast) -> Vec3 {
    for (i, p) in s.players.iter_mut().enumerate() {
        (p.bot, p.p, p.shield) = (false, [6.7, crate::feast::FLOOR, 0.2 + 0.35 * i as f32], 0.);
    }
    let (spot, yaw) = ([3., crate::feast::FLOOR, 1.], 0.7f32);
    let back = Vec3::new(yaw.sin(), 0., yaw.cos()) * 0.14;
    s.players[1].p = spot;
    s.players[1].yaw = yaw;
    s.players[0].p = [spot[0] + back.x, spot[1], spot[2] + back.z];
    s.players[0].cooldown = 0.;
    s.embody();
    (chest(&s.players[1]) - muzzle(&s.players[0])).normalize()
}
/// A bare counter seven metres by three, played as `mode` (a free for all for the famine).
fn ring(mode: &str) -> Feast {
    let flat = Map::parse(r#"{"format": "hotdog-map", "version": 1, "name": "Ring", "bounds": [0, 0, 7, 3], "starts": [{"p": [0.3, 1]}], "spawns": [{"p": [6.9, 0.1]}]}"#).unwrap();
    let mut s = Feast::with_map(flat, false);
    match mode {
        "famine" => s.begin_famine(),
        "ffa" => s.set_rules(Rules::parse("mode=ffa")),
        "fight" => {
            // (The fight's rule for who may be struck, on a counter with no forcefield: two teams.)
            s.stage = Stage::Fight;
            s.players[1].team = 1;
        }
        _ => {}
    }
    s
}

#[test]
fn f_is_the_strike_along_the_aim_in_every_gun_mode_a_kill_from_behind_half_from_the_front_and_the_healing_bite_and_eating_are_the_famines_alone() {
    for mode in ["famine", "ffa", "fight"] {
        // From behind, along the aim: a kill (the famine's bite is the same blow, and feeds the biter).
        let mut s = ring(mode);
        let d = behind(&mut s);
        s.players[0].hp = 40.;
        assert!(s.melee(1, d), "{mode}");
        let (k, v) = (s.player(1).unwrap().clone(), s.player(2).unwrap().clone());
        assert!(!v.alive && v.last_damage.kind == BACKSTAB && v.last_damage.attacker == 1, "{mode}: {:?}", v.last_damage);
        assert_eq!((k.meals > 0, k.hp > 40.), (mode == "famine", mode == "famine"), "{mode}: only the famine's bite heals");
        // From the front: half of MAX_HP.
        let mut s = ring(mode);
        behind(&mut s);
        s.players[1].yaw += std::f32::consts::PI;
        s.embody();
        let d = (chest(&s.players[1]) - muzzle(&s.players[0])).normalize();
        assert!(s.melee(1, d));
        assert_eq!((s.player(2).unwrap().hp, s.player(2).unwrap().last_damage.kind), (50., MELEE), "{mode}");
        // E eats a corpse in the famine alone.
        let mut s = ring(mode);
        behind(&mut s);
        s.hurt(2, 500.);
        assert_eq!(s.eat(1), mode == "famine", "{mode}");
    }
    // Outside the famine there is no bite: a hotdog at its feet but off the aim is not touched, and nothing heals.
    for mode in ["ffa", "fight"] {
        let mut s = ring(mode);
        let d = behind(&mut s);
        s.players[0].hp = 40.;
        assert!(!s.bite(1), "{mode}");
        assert!(s.melee(1, -d), "{mode}: a strike at thin air is still a strike");
        assert_eq!((s.player(2).unwrap().hp, s.player(1).unwrap().hp, s.player(1).unwrap().meals), (100., 40., 0), "{mode}");
    }
    // In the wave F strikes cats, and no hotdog.
    let mut s = ring("");
    s.stage = Stage::Wave;
    let d = behind(&mut s);
    assert!(s.melee(1, d));
    assert_eq!(s.player(2).unwrap().hp, 100.);
    // The bots of Free For All strike and shoot, and never eat: a bot whose prey is a corpse at its feet passes it by.
    let mut s = game("counter-siege", Some(Mode::Ffa));
    let mut eaten = false;
    for _ in 0..(60. / DT) as usize {
        s.tick(DT);
        eaten |= s.players.iter().any(|p| p.consumed || p.meals > 0);
    }
    assert!(!eaten, "nobody eats or bites in Free For All");
}

#[test]
fn killing_a_hotdog_scores_a_kill_and_100_in_every_gun_mode_and_only_free_for_all_brings_the_dead_back() {
    for mode in ["famine", "ffa", "fight"] {
        let mut s = ring(mode);
        behind(&mut s);
        let (o, t) = (muzzle(&s.players[0]), chest(&s.players[1]));
        s.harm(2, 500., true, Damage::by(kind_of(RIFLE), 1, o, t - o));
        let (k, v) = (s.player(1).unwrap(), s.player(2).unwrap());
        assert!(!v.alive, "{mode}");
        assert_eq!((k.kills, k.score), (1, 100), "{mode}");
        assert_eq!((v.deaths, v.respawn > 0.), if mode == "ffa" { (1, true) } else { (0, false) }, "{mode}");
    }
    // In the real thing: a Forts fight between bots scores the kills its rounds make.
    let mut s = fight("pillow-fort", None);
    run(&mut s, 60.);
    let dead = s.players.iter().filter(|p| !p.alive).count() as u32;
    let kills: u32 = s.players.iter().map(|p| p.kills).sum();
    assert!(dead == 0 || kills > 0, "{dead} dead, {kills} kills");
}

#[test]
fn every_gun_mode_starts_on_every_map_it_says_it_can_be_played_on_and_says_why_it_cannot_on_the_rest() {
    let mut table = vec![];
    for (name, map) in maps() {
        let dojo = map.mode == crate::kungfu::MODE;
        let made = Mode::made_for(&map);
        for mode in Mode::GUNS {
            let refused = map.refuses(mode);
            // Free For All and the siege on every map but the dojo; Forts on the maps with its layout (the Forts maps).
            let expected = !dojo && (mode != Mode::Forts || made == Mode::Forts);
            assert_eq!(refused.is_none(), expected, "{name} as {mode:?}: {refused:?}");
            table.push((name.clone(), mode, refused.is_none()));
            let Some(why) = refused else {
                let mut s = game_on(map.clone(), Some(mode));
                assert_eq!(s.mode(), mode, "{name}");
                assert_eq!(s.players.len(), if mode == Mode::Forts { FORT_PLAYERS } else { 5 }, "{name} as {mode:?}");
                assert_eq!(s.fort.is_some(), mode == Mode::Forts, "{name} as {mode:?}");
                let first = match mode {
                    Mode::Forts => Stage::Build,
                    Mode::Ffa => Stage::FreeForAll,
                    _ => Stage::Warmup,
                };
                assert_eq!(s.stage, first, "{name} as {mode:?}");
                assert!(s.players.iter().all(loaded), "{name} as {mode:?}");
                // It plays: a few seconds, and the next round is the same mode.
                run(&mut s, 3.);
                s.reset();
                assert_eq!((s.mode(), s.fort.is_some()), (mode, mode == Mode::Forts), "{name} as {mode:?}, next round");
                continue;
            };
            assert!(why.starts_with(&map.name), "{why}");
            assert!(why.contains(if dojo { "Kung Fu" } else { "Forts layout" }), "{why}");
        }
        // The dojo is Kung Fu's.
        assert_eq!(map.refuses(Mode::KungFu).is_none(), dojo, "{name}");
    }
    // Thirteen gun maps, four of them with the Forts layout.
    let count = |mode: Mode| table.iter().filter(|(_, m, ok)| *m == mode && *ok).count();
    assert_eq!((count(Mode::Siege), count(Mode::Ffa), count(Mode::Forts)), (13, 13, 4), "{table:?}");
}

#[test]
fn the_mode_is_the_games_and_a_maps_tag_only_says_which_it_plays_when_nothing_else_does() {
    // The rules name the siege and Forts as they name the rest, and write them back.
    for (line, mode) in [("mode=siege", Mode::Siege), ("mode=forts", Mode::Forts), ("mode=ffa", Mode::Ffa), ("mode=cards", Mode::Cards), ("mode=kungfu", Mode::KungFu)] {
        let r = Rules::parse(line);
        assert_eq!((r.mode(), r.text().as_str()), (Some(mode), line));
        assert_eq!(Rules::parse(&r.text()), r);
        assert_eq!(Mode::named(mode.name()), Some(mode));
    }
    assert_eq!(Rules::parse("").mode(), None);
    // A game in the map's own mode says nothing of it (what a game always said); in another, it does.
    assert_eq!(Rules::playing(Mode::Siege, &map("counter-siege")), Rules::default());
    assert_eq!(Rules::playing(Mode::Forts, &map("pillow-fort")), Rules::default());
    assert_eq!(Rules::playing(Mode::Siege, &map("pillow-fort")).text(), "mode=siege");
    assert_eq!(Rules::playing(Mode::Ffa, &map("pillow-fort")).text(), "mode=ffa");
    // A Forts map is played as Forts by default, and as a siege when the rules say so: five hotdogs at its starts, no
    // teams, no forcefield, the siege's warm-up, and its zones passed over; the next round too.
    let mut s = game("pillow-fort", None);
    assert_eq!((s.mode(), s.players.len(), s.stage), (Mode::Forts, 6, Stage::Build));
    s.set_rules(Rules::parse("mode=siege"));
    assert_eq!((s.mode(), s.players.len(), s.stage, s.fort.is_none()), (Mode::Siege, 5, Stage::Warmup, true));
    assert_eq!(s.alert, crate::feast::SIEGE_ALERT);
    assert!(s.players.iter().all(|p| p.team == 0 && loaded(p)) && !s.allied(1, 3));
    run(&mut s, 13.);
    assert_eq!(s.stage, Stage::Wave, "the wave comes out of its microwave");
    assert_eq!(s.wave(), 2 * s.level.map.spawns.len() as u32, "two cats a spawn point (at least six)");
    s.reset();
    assert_eq!((s.mode(), s.players.len(), s.stage), (Mode::Siege, 5, Stage::Warmup));
    // And back to its own mode.
    s.set_rules(Rules::default());
    assert_eq!((s.mode(), s.players.len(), s.stage), (Mode::Forts, 6, Stage::Build));
    // A siege map is not made into a Forts map: asked for Forts, it plays its own mode.
    let mut s = game("counter-siege", None);
    s.set_rules(Rules::parse("mode=forts"));
    assert_eq!((s.mode(), s.fort.is_none(), s.players.len()), (Mode::Siege, true, 5));
    // A map made for a siege that has the whole Forts layout may be played as Forts; with part of it, not.
    let mut text: serde_json::Value = serde_json::from_str(&read("assets/maps/pillow-fort.json")).unwrap();
    text["mode"] = serde_json::json!("siege");
    let laid = Map::from_value(&text).unwrap();
    assert!(laid.fort_layout() && laid.refuses(Mode::Forts).is_none());
    let s = game_on(laid.clone(), None);
    assert_eq!((s.mode(), s.players.len()), (Mode::Siege, 5));
    let s = game_on(laid, Some(Mode::Forts));
    assert_eq!((s.mode(), s.players.len(), s.stage), (Mode::Forts, 6, Stage::Build));
    text["zones"].as_array_mut().unwrap().retain(|z| z["kind"] != "bacon");
    let part = Map::from_value(&text).unwrap();
    assert!(!part.fort_layout() && part.fortress().is_none());
    assert!(part.refuses(Mode::Forts).unwrap().contains("no Forts layout"));
}

#[test]
fn the_build_time_is_the_rules_150_seconds_unless_the_server_or_the_page_say_otherwise_and_not_the_maps() {
    let mut text: serde_json::Value = serde_json::from_str(&read("assets/maps/pillow-fort.json")).unwrap();
    text["build"] = serde_json::json!(40);
    let short = Map::from_value(&text).unwrap();
    assert_eq!(short.build, 40., "read (and written back)");
    let mut s = game_on(short.clone(), None);
    assert_eq!(s.fort_state().unwrap().build, BUILD as f32, "and passed over");
    // The rules' time (--build, ?build=) lasts: the next round, and a Forts round set out after a siege.
    s.set_build(30.);
    s.reset();
    assert_eq!(s.fort_state().unwrap().build, 30.);
    s.set_rules(Rules::parse("mode=siege"));
    s.set_rules(Rules::default());
    assert_eq!((s.mode(), s.fort_state().unwrap().build, s.fort_state().unwrap().ends), (Mode::Forts, 30., s.elapsed + 30.));
    // Set before the rules ask for Forts (the server's order), it is still the build's.
    let mut s = Feast::with_map(map("counter-siege"), false);
    s.set_build(20.);
    let mut s2 = game_on(short, Some(Mode::Siege));
    s2.set_build(25.);
    s2.set_rules(Rules::parse("mode=forts"));
    assert_eq!(s2.fort_state().unwrap().build, 25.);
    assert!(s.fort.is_none());
}

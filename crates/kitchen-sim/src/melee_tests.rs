// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The melee blow by angle: struck from inside a hotdog's (or a zombie's) rear arc, 60 degrees either side of
//! directly behind where it faces, it kills (and goes on the record as BACKSTAB); struck from anywhere else it takes
//! half of the most HP there is. The same blow in a Forts fight and in Free For All (a thrust along the aim, F in every
//! gun mode) and in the siege's famine (the bite, which heals), against a hotdog standing, crouching or prone, carrying
//! or stimmed, and from a bot.
use super::*;

const DT: f32 = 1. / 30.;
/// How far apart the two hotdogs stand: the attacker's muzzle is in reach of the victim's chest (RANGE[0] 0.21 m
/// along the aim, to a sphere of 0.055 m) and the victim is within a bite's 0.17 m.
const APART: f32 = 0.14;
/// Where the pair stand, and the way the victim faces (an angle that lies along neither axis).
const SPOT: [f32; 3] = [3., FLOOR, 1.];
const FACING: f32 = 0.7;

/// Where the attacker and the victim stand (x, z), the attacker being `degrees` round from directly behind the victim
/// (which faces FACING), APART from it.
fn places(degrees: f32) -> ([f32; 2], [f32; 2]) {
    // Directly behind the victim is (sin yaw, cos yaw); the attacker is `degrees` round from it.
    let (sin, cos) = degrees.to_radians().sin_cos();
    let behind = Vec3::new(FACING.sin(), 0., FACING.cos());
    let round = Vec3::new(behind.x * cos + behind.z * sin, 0., -behind.x * sin + behind.z * cos);
    ([SPOT[0] + round.x * APART, SPOT[2] + round.z * APART], [SPOT[0], SPOT[2]])
}
/// A bare counter seven metres by three, with a pillow on it at `pillow` (for something to carry). Hotdog 1 (the
/// attacker) has the syrup rifle in hand and hotdog 2 (the victim) stands where `places` puts them. `stage` is the Forts
/// fight (the two on different teams) or the free for all. Nobody is a bot, and the rest are far away.
fn pair_with(stage: Stage, degrees: f32, pillow: [f32; 2]) -> Feast {
    let map = Map::parse(&format!(
        r#"{{"format": "hotdog-map", "version": 1, "name": "Ring", "bounds": [0, 0, 7, 3], "starts": [{{"p": [0.3, 1]}}], "spawns": [{{"p": [6.9, 0.1]}}], "props": [{{"kind": "pillow", "p": [{}, {}], "base": 0, "yaw": 0}}]}}"#,
        pillow[0], pillow[1]
    ))
    .unwrap();
    let mut s = Feast::with_map(map, false);
    s.begin_famine();
    s.stage = stage;
    for (i, p) in s.players.iter_mut().enumerate() {
        (p.bot, p.p, p.team) = (false, [6.7, FLOOR, 0.2 + 0.35 * i as f32], 0);
    }
    let (attacker, victim) = places(degrees);
    s.players[1].team = 1;
    (s.players[1].p, s.players[1].yaw) = ([victim[0], FLOOR, victim[1]], FACING);
    s.players[0].p = [attacker[0], FLOOR, attacker[1]];
    s.embody();
    s
}
fn pair(stage: Stage, degrees: f32) -> Feast {
    pair_with(stage, degrees, [0.5, 2.5])
}
/// Hotdog 1 strikes at hotdog 2's chest from where it stands (a bite in the free for all aims at nothing, and does not
/// need to). Whether the blow was made.
fn strike(s: &mut Feast) -> bool {
    s.players[0].cooldown = 0.;
    s.players[0].bite_cooldown = 0.;
    let (from, to) = (muzzle(&s.players[0]), chest(&s.players[1]));
    s.melee(1, (to - from).normalize())
}
/// Hotdog `id` takes the pillow in its hands (it is looking at it).
fn take_the_pillow(s: &mut Feast, id: u32) -> bool {
    let (at, p) = (s.props.position(0), s.player(id).unwrap());
    let d = (Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32) - muzzle(p)).normalize();
    s.grab(id, d)
}
fn victim(s: &Feast) -> &Player {
    s.player(2).unwrap()
}
const STAGES: [Stage; 2] = [Stage::Fight, Stage::FreeForAll];

#[test]
fn a_blow_from_directly_behind_kills_and_goes_on_the_record_as_a_backstab() {
    for stage in STAGES {
        let mut s = pair(stage, 0.);
        assert!(strike(&mut s), "{stage:?}");
        let (v, k) = (victim(&s), s.player(1).unwrap());
        assert!(!v.alive && v.hp == 0., "{stage:?}: {} HP left", v.hp);
        assert_eq!((v.last_damage.kind, v.last_damage.attacker, v.last_damage.amount), (BACKSTAB, 1, 100.), "{stage:?}");
        // It came from where the attacker stood, along the blow.
        assert!(Vec3::from_array(v.last_damage.p).distance(Vec3::from_array(k.p)) < 1e-5);
        // (A bite is level: it comes from where the biter stood, across the counter.)
        let (along, aimed) = (Vec3::from_array(v.last_damage.d), chest(v) - muzzle(k));
        let aimed = if stage == Stage::Fight { aimed } else { Vec3::new(aimed.x, 0., aimed.z) }.normalize();
        assert!(along.dot(aimed) > 0.999, "{stage:?}: {along:?} against {aimed:?}");
        assert_eq!(k.swings, 1);
    }
    // In Free For All (played by its rules) F is the strike along the aim, not the bite: the kill is the attacker's,
    // counted and scored as a kill (no meal), and the victim will be back.
    let mut s = pair(Stage::FreeForAll, 0.);
    s.rules = crate::rules::Rules { ffa: true, ..Default::default() };
    assert!(strike(&mut s));
    let (k, v) = (s.player(1).unwrap(), victim(&s));
    assert_eq!((k.kills, k.score, k.meals, v.deaths, v.last_damage.kind), (1, 100, 0, 1, BACKSTAB));
    let (along, aimed) = (Vec3::from_array(v.last_damage.d), (chest(v) - muzzle(k)).normalize());
    assert!(along.dot(aimed) > 0.999, "along the aim: {along:?} against {aimed:?}");
    assert!(v.respawn > 0.);
    // In the siege's famine the bite's kill scores the kill too, and the meal on top.
    let mut s = pair(Stage::FreeForAll, 0.);
    assert!(strike(&mut s));
    let (k, v) = (s.player(1).unwrap(), victim(&s));
    assert_eq!((k.kills, k.score, k.meals, v.deaths), (1, 125, 1, 0), "no respawn in the siege: no death counted");
}
#[test]
fn a_blow_from_the_front_takes_half_of_the_most_hp_there_is_and_a_second_one_kills() {
    assert_eq!((PLAYER_DAMAGE[0], ZOMBIE_DAMAGE[0]), (MAX_HP / 2., ZOMBIE_HP / 2.));
    assert_eq!((MAX_HP, ZOMBIE_HP), (100., 70.));
    for stage in STAGES {
        let mut s = pair(stage, 180.);
        assert!(strike(&mut s), "{stage:?}");
        let v = victim(&s);
        assert!(v.alive && v.hp == 50., "{stage:?}: {} HP left", v.hp);
        assert_eq!((v.last_damage.kind, v.last_damage.attacker, v.last_damage.amount), (MELEE, 1, 50.), "{stage:?}");
        // The second one finishes it: a kill, but not a backstab.
        assert!(strike(&mut s));
        let v = victim(&s);
        assert!(!v.alive && v.hp == 0. && v.last_damage.kind == MELEE && v.last_damage.amount == 50., "{stage:?}");
    }
    // A hotdog already hurt is not spared the half it has not got: from the front it takes what is left of it...
    let mut s = pair(Stage::Fight, 180.);
    s.players[1].hp = 30.;
    assert!(strike(&mut s));
    assert!(!victim(&s).alive && victim(&s).last_damage.amount == 30. && victim(&s).last_damage.kind == MELEE);
    // ...and from behind a blow takes all of it, whatever it is.
    for hp in [100., 63.5, 1., 0.5] {
        let mut s = pair(Stage::Fight, 10.);
        s.players[1].hp = hp;
        assert!(strike(&mut s));
        assert!(!victim(&s).alive && victim(&s).last_damage.kind == BACKSTAB && victim(&s).last_damage.amount == hp, "{hp}");
    }
}
#[test]
fn the_rear_arc_is_sixty_degrees_either_side_of_directly_behind() {
    // 50 degrees off directly behind kills, 70 degrees only hurts, on either side and in either mode.
    for stage in STAGES {
        for degrees in [0., 30., 50., 59., -50., -59.] {
            let mut s = pair(stage, degrees);
            assert!(strike(&mut s));
            assert!(!victim(&s).alive && victim(&s).last_damage.kind == BACKSTAB, "{stage:?}: {degrees} degrees off directly behind");
        }
        for degrees in [61., 70., 90., 120., 179., 180., -61., -70., -90., -120.] {
            let mut s = pair(stage, degrees);
            assert!(strike(&mut s));
            assert!(victim(&s).alive && victim(&s).hp == 50. && victim(&s).last_damage.kind == MELEE, "{stage:?}: {degrees} degrees off directly behind: {} HP", victim(&s).hp);
        }
    }
    // The rule itself, flat: it does not matter how high the attacker is, or where the victim looks up or down.
    let victim_at = [1., FLOOR, 1.];
    assert!(backstab(victim_at, 0., Vec3::new(1., FLOOR + 0.5, 1.2), Vec3::NEG_Z), "a victim facing -z has +z behind it");
    assert!(!backstab(victim_at, 0., Vec3::new(1., FLOOR, 0.8), Vec3::Z), "and -z in front of it");
    assert!(backstab(victim_at, std::f32::consts::PI, Vec3::new(1., FLOOR, 0.8), Vec3::Z), "turned round, the other way");
    assert!(backstab(victim_at, -std::f32::consts::FRAC_PI_2, Vec3::new(0.8, FLOOR, 1.), Vec3::X), "facing +x: -x is behind");
    assert!(!backstab(victim_at, -std::f32::consts::FRAC_PI_2, Vec3::new(1.2, FLOOR, 1.), Vec3::NEG_X));
    // On top of it, there is no way to measure: the way the blow was aimed stands in for the way the attacker faces.
    assert!(backstab(victim_at, 0., Vec3::from_array(victim_at), Vec3::NEG_Z), "both looking along -z: one behind the other");
    assert!(!backstab(victim_at, 0., Vec3::from_array(victim_at), Vec3::Z), "face to face");
    assert!(!backstab(victim_at, 0., Vec3::from_array(victim_at), Vec3::Y), "and nothing to go by at all is not a backstab");
}
#[test]
fn facing_is_the_victims_yaw_alone_not_its_pitch_nor_where_the_attacker_looks() {
    for pitch in [-1., 0., 1.2] {
        let mut s = pair(Stage::Fight, 20.);
        s.players[1].pitch = pitch;
        assert!(strike(&mut s));
        assert!(!victim(&s).alive && victim(&s).last_damage.kind == BACKSTAB, "pitch {pitch}");
        let mut s = pair(Stage::Fight, 160.);
        s.players[1].pitch = pitch;
        assert!(strike(&mut s));
        assert!(victim(&s).alive && victim(&s).hp == 50., "pitch {pitch}");
    }
    // The attacker's own yaw and pitch are only what its aim says: struck from behind, the blow kills whichever way
    // its hotdog is turned.
    let mut s = pair(Stage::Fight, 0.);
    s.players[0].yaw = 2.;
    assert!(strike(&mut s));
    assert_eq!(victim(&s).last_damage.kind, BACKSTAB);
    // A victim turning round (yaw is the only thing that turns) gets the other side of the same blow.
    let mut s = pair(Stage::Fight, 0.);
    s.players[1].yaw += std::f32::consts::PI;
    assert!(strike(&mut s));
    assert!(victim(&s).alive && victim(&s).hp == 50.);
}
#[test]
fn crouching_prone_and_carrying_change_neither_the_arc_nor_the_blow() {
    // Any stance of the victim, any stance of the attacker (the blow goes from its muzzle to the victim's chest).
    let stance = |p: &mut Player, how: usize| (p.crouch, p.prone) = (how == 1, how == 2);
    for stage in STAGES {
        for (theirs, ours) in (0..3).flat_map(|a| (0..3).map(move |b| (a, b))) {
            for (degrees, kills) in [(0., true), (45., true), (75., false), (180., false)] {
                let mut s = pair(stage, degrees);
                stance(&mut s.players[1], theirs);
                stance(&mut s.players[0], ours);
                s.embody();
                assert!(strike(&mut s), "{stage:?}: victim {theirs} attacker {ours} at {degrees}");
                let v = victim(&s);
                if kills {
                    assert!(!v.alive && v.last_damage.kind == BACKSTAB, "{stage:?}: victim {theirs} attacker {ours} at {degrees}");
                } else {
                    assert!(v.alive && v.hp == 50. && v.last_damage.kind == MELEE, "{stage:?}: victim {theirs} attacker {ours} at {degrees}: {} HP", v.hp);
                }
            }
        }
    }
    // A victim carrying a loose object is struck all the same, and drops what it carried when it dies.
    for (degrees, kills) in [(0., true), (180., false)] {
        let (_, victim_at) = places(degrees);
        let mut s = pair_with(Stage::Fight, degrees, [victim_at[0], victim_at[1] - 0.25]);
        assert!(take_the_pillow(&mut s, 2), "the victim takes the pillow");
        assert_eq!(s.players[1].carry, 0);
        s.players[1].yaw = FACING;
        s.embody();
        assert!(strike(&mut s), "{degrees}");
        let v = victim(&s);
        assert_eq!(v.alive, !kills, "{degrees}: {} HP", v.hp);
        assert_eq!(v.last_damage.kind, if kills { BACKSTAB } else { MELEE });
        assert_eq!(v.carry >= 0, !kills, "{degrees}: a dead hotdog drops what it carried");
    }
    // An attacker with its hands full cannot strike, from either side: no gun is in hand to strike with.
    for degrees in [0., 180.] {
        let (attacker_at, _) = places(degrees);
        let mut s = pair_with(Stage::Fight, degrees, [attacker_at[0], attacker_at[1] - 0.25]);
        assert!(take_the_pillow(&mut s, 1), "the attacker takes the pillow");
        assert!(!strike(&mut s), "{degrees}");
        assert_eq!((victim(&s).hp, victim(&s).last_damage.kind, s.players[0].swings), (100., 0, 0));
    }
}
#[test]
fn a_stimmed_hotdog_takes_the_blows_doubled_as_it_takes_everything_and_a_backstab_still_kills_it() {
    // (Stim doubles what rivals do: a blow from the front takes all 100, a hotdog that is stimmed is not tougher.)
    let mut s = pair(Stage::Fight, 180.);
    s.players[1].stim = 5.;
    assert!(strike(&mut s));
    assert!(!victim(&s).alive && victim(&s).last_damage.kind == MELEE && victim(&s).last_damage.amount == 100.);
    // A tuning that made stim a protection leaves the backstab a kill all the same; one that made it immunity does not.
    for (stim, dies) in [(0.5, true), (1., true), (3., true), (0., false)] {
        let mut s = pair(Stage::Fight, 0.);
        s.players[1].stim = 5.;
        s.tune.stim_damage = stim;
        assert!(strike(&mut s));
        assert_eq!(!victim(&s).alive, dies, "stim ×{stim}");
        assert_eq!(victim(&s).last_damage.kind, BACKSTAB);
    }
    // A hotdog on its spawn protection is spared the blow, which is on its record (a backstab, for what it would have taken).
    let mut s = pair(Stage::FreeForAll, 0.);
    s.players[1].shield = 3.;
    assert!(strike(&mut s));
    assert!(victim(&s).alive && victim(&s).hp == 100. && victim(&s).last_damage.kind == BACKSTAB && victim(&s).last_damage.amount == 100.);
}
#[test]
fn the_tuning_names_the_front_blow_and_leaves_the_backstab_alone() {
    let base = crate::tune::Tuning::default();
    assert_eq!((base.get("melee.hotdog"), base.get("melee.zombie")), (Some(50.), Some(35.)));
    assert_eq!(base.text(), "", "the default has nothing to say");
    let mut s = pair(Stage::Fight, 180.);
    s.tune.hotdog[0] = 20.;
    assert!(strike(&mut s));
    assert_eq!(victim(&s).hp, 80.);
    let mut s = pair(Stage::Fight, 0.);
    s.tune.hotdog[0] = 20.;
    assert!(strike(&mut s));
    assert!(!victim(&s).alive, "the tuned damage is for the front: a backstab kills");
    let mut tuned = crate::tune::Tuning::default();
    assert!(tuned.set("melee.hotdog", 30.));
    assert_eq!(tuned.hotdog[0], 30.);
    assert_eq!(tuned.text(), "melee.hotdog=30");
}
#[test]
fn a_bite_in_the_free_for_all_is_the_same_blow_and_feeds_the_biter_as_it_always_has() {
    // From the front: half the victim's HP, and the biter heals on the first 24 of it, as before.
    let mut s = pair(Stage::FreeForAll, 180.);
    s.players[0].hp = 40.;
    assert!(strike(&mut s));
    let (k, v) = (s.player(1).unwrap(), victim(&s));
    assert!(v.alive && v.hp == 50. && v.last_damage.kind == MELEE);
    assert!((k.hp - (40. + 24. * 0.8)).abs() < 1e-4 && k.meals == 1 && k.bite_cooldown > 0., "{} HP, {} meals", k.hp, k.meals);
    // From behind it kills, and feeds the biter on no more than a bite did.
    let mut s = pair(Stage::FreeForAll, 0.);
    s.players[0].hp = 40.;
    assert!(strike(&mut s));
    let (k, v) = (s.player(1).unwrap(), victim(&s));
    assert!(!v.alive && v.last_damage.kind == BACKSTAB);
    assert!((k.hp - (40. + 24. * 0.8)).abs() < 1e-4 && k.meals == 1, "{} HP", k.hp);
    // The corpse can still be eaten (E), and is not a blow.
    let swings = s.player(1).unwrap().swings;
    s.players[0].bite_cooldown = 0.;
    assert!(s.eat(1));
    assert_eq!(s.player(1).unwrap().swings, swings + 1);
    // Nothing is a backstab when nothing was struck: a bite at thin air, a blow at a team mate.
    let mut s = pair(Stage::Fight, 0.);
    s.players[1].team = 0;
    assert!(strike(&mut s));
    assert!(victim(&s).alive && victim(&s).hp == 100. && victim(&s).last_damage.kind == 0);
}
#[test]
fn a_bot_strikes_by_the_same_rule() {
    // A bot with its victim at its feet, in the free for all, bites: from behind that is a backstab.
    for (facing, kills) in [(FACING, true), (FACING + std::f32::consts::PI, false)] {
        let mut s = pair(Stage::FreeForAll, 0.);
        // (Hotdog 2 is the bot now, and the other way round: it is the one that stands behind hotdog 1.)
        s.players.swap(0, 1);
        s.players[0].id = 1;
        s.players[1].id = 2;
        s.players[1].bot = true;
        s.players[0].yaw = facing;
        s.players[0].hp = 100.;
        s.embody();
        let bot = s.players[1].clone();
        let d = Vec3::from_array(s.players[0].p) - Vec3::from_array(bot.p);
        assert!(d.length() < 0.15);
        s.face(2, d);
        s.players[1].cooldown = 0.;
        assert!(s.melee(2, (chest(&s.players[0]) - muzzle(&s.players[1])).normalize()));
        let v = s.player(1).unwrap();
        assert_eq!(!v.alive, kills, "yaw {facing}: {} HP", v.hp);
        assert_eq!((v.last_damage.attacker, v.last_damage.kind), (2, if kills { BACKSTAB } else { MELEE }));
    }
}
#[test]
fn a_zombie_struck_from_behind_dies_and_from_the_front_takes_half_of_what_it_has() {
    // A formed zombie 0.12 m in front of hotdog 1 (towards +z), facing `yaw`, at full health.
    let brawl = |yaw: f32| {
        let mut s = Feast::new(false);
        s.stage = Stage::Wave;
        s.wave_spawned = WAVE;
        for (i, p) in s.players.iter_mut().enumerate() {
            p.bot = false;
            p.p = [-1.5 + i as f32 * 0.1, FLOOR, 3.55];
        }
        s.players[0].p = [0., FLOOR, 2.4];
        s.enemies.push(Zombie { id: 1, p: [0., FLOOR, 2.52], yaw, hp: ZOMBIE_HP, born: 0., forming: 0. });
        s
    };
    // A zombie that faces the hotdog (a zombie's yaw is the way it looks: yaw 0 looks along -z, at it) is struck from
    // the front, twice to kill.
    let mut s = brawl(0.);
    assert!(s.melee(1, Vec3::Z));
    assert_eq!(s.enemies[0].hp, ZOMBIE_HP / 2.);
    s.players[0].cooldown = 0.;
    assert!(s.melee(1, Vec3::Z));
    assert!(s.enemies.is_empty());
    assert_eq!((s.players[0].kills, s.wave_kills), (1, 1));
    // One that looks the other way dies at the first blow, and the kill is the striker's.
    let mut s = brawl(std::f32::consts::PI);
    assert!(s.melee(1, Vec3::Z));
    assert!(s.enemies.is_empty());
    assert_eq!((s.players[0].kills, s.wave_kills, s.players[0].score), (1, 1, 100));
    // The edges of its arc, as a hotdog's.
    for (turn, kills) in [(50., true), (-50., true), (70., false), (-70., false)] {
        let mut s = brawl(std::f32::consts::PI + f32::to_radians(turn));
        assert!(s.melee(1, Vec3::Z));
        assert_eq!(s.enemies.is_empty(), kills, "turned {turn} degrees from facing away");
        if !kills {
            assert_eq!(s.enemies[0].hp, ZOMBIE_HP / 2.);
        }
    }
    // A zombie of a million HP (a check's way of making one that cannot be killed) is hit and not killed from behind.
    let mut s = brawl(std::f32::consts::PI);
    s.enemies[0].hp = 1e6;
    assert!(s.melee(1, Vec3::Z));
    assert_eq!(s.enemies.len(), 1);
    // The tuning's zombie damage is the front blow.
    let mut s = brawl(0.);
    s.tune.zombie[0] = 10.;
    assert!(s.melee(1, Vec3::Z));
    assert_eq!(s.enemies[0].hp, ZOMBIE_HP - 10.);
}
#[test]
fn no_other_kind_of_damage_is_a_backstab() {
    // A round from behind is a round: the rule is for the blow in the hand.
    let mut s = pair(Stage::FreeForAll, 0.);
    s.players[0].p = [SPOT[0] + FACING.sin() * 0.3, FLOOR, SPOT[2] + FACING.cos() * 0.3];
    s.embody();
    s.players[0].cooldown = 0.;
    let (from, to) = (muzzle(&s.players[0]), chest(&s.players[1]));
    assert!(s.fire(1, (to - from).normalize(), RIFLE));
    for _ in 0..60 {
        s.tick(DT);
    }
    let v = victim(&s);
    assert!(v.alive || v.last_damage.kind != BACKSTAB);
    assert_ne!(v.last_damage.kind, BACKSTAB);
    assert_eq!([MELEE, BACKSTAB], [6, 14]);
}

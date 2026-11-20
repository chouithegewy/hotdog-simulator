// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The interface from the outside, as a C caller uses it: null and garbage handles and inputs are errors and never
//! crashes, the offline modes play, the same seed plays the same game, and an online client kept from frames encoded
//! the way the server encodes them (net.rs `encode_frame` against what the client acknowledged) sees what the server has.
use super::*;
use kitchen_sim::feast::Feast;
use kitchen_sim::map::Map;
use kitchen_sim::net::{Batch, Events, Frame, Header, Loose, Mine, Seat, State as Wire};
use std::ptr::{null, null_mut};

fn s(t: &str) -> KsStr {
    KsStr::of(t)
}
fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn string(k: KsStr) -> String {
    if k.len == 0 {
        return String::new();
    }
    String::from_utf8(unsafe { std::slice::from_raw_parts(k.ptr, k.len) }.to_vec()).unwrap()
}
fn error() -> String {
    string(ks_last_error())
}
fn create(map: &str, rules: &str, seed: u64, flags: u32) -> *mut KsGame {
    let mut g = null_mut();
    let status = unsafe { ks_game_create(s(map), s(rules), seed, flags, &mut g) };
    assert_eq!(status, KsStatus::KS_OK, "{}", error());
    assert!(!g.is_null());
    g
}
fn players(g: *const KsGame) -> Vec<KsPlayer> {
    let mut n = 0;
    assert_eq!(unsafe { ks_game_players(g, null_mut(), 0, &mut n) }, KsStatus::KS_OK, "{}", error());
    let mut out = vec![unsafe { std::mem::zeroed::<KsPlayer>() }; n as usize];
    assert_eq!(unsafe { ks_game_players(g, out.as_mut_ptr(), n, &mut n) }, KsStatus::KS_OK);
    out
}
fn events(g: *mut KsGame) -> Vec<KsEvent> {
    let mut all = vec![];
    let mut chunk = [KsEvent::default(); 7];
    loop {
        let mut n = 0;
        assert_eq!(unsafe { ks_game_events(g, chunk.as_mut_ptr(), chunk.len() as u32, &mut n) }, KsStatus::KS_OK);
        all.extend_from_slice(&chunk[..n as usize]);
        if (n as usize) < chunk.len() {
            return all;
        }
    }
}
fn matched(g: *const KsGame) -> KsMatch {
    let mut m = unsafe { std::mem::zeroed::<KsMatch>() };
    assert_eq!(unsafe { ks_game_match(g, &mut m) }, KsStatus::KS_OK, "{}", error());
    m
}
fn command(stick: [f32; 2], yaw: f32, buttons: u32) -> KsCommand {
    KsCommand { stick_x: stick[0], stick_y: stick[1], yaw, pitch: 0., buttons, weapon: 0, pickup: -1 }
}
fn step(g: *mut KsGame, c: &KsCommand, dt: f32) {
    unsafe {
        assert_eq!(ks_game_set_command(g, c), KsStatus::KS_OK, "{}", error());
        assert_eq!(ks_game_step(g, dt), KsStatus::KS_OK, "{}", error());
    }
}

#[test]
fn the_version_and_the_static_tables() {
    let v = ks_version();
    assert_eq!((v.abi, v.protocol, v.tick_hz, v.send_hz), (KS_ABI_VERSION, 12, 60, 30));
    assert_eq!(string(v.version), env!("CARGO_PKG_VERSION"));
    assert_eq!(ks_protocol(), 12);
    assert_eq!(ks_kungfu_move_count(), 85);
    let mut m = unsafe { std::mem::zeroed::<KsMove>() };
    assert_eq!(unsafe { ks_kungfu_move(15, &mut m) }, KsStatus::KS_OK);
    assert_eq!((string(m.key).as_str(), string(m.name).as_str(), m.kind, m.style, m.slot), ("TigerLight1", "Tiger Claw Swipe", KS_MOVE_LIGHT, 0, 0));
    assert_eq!(&m.chains[..m.chain_count as usize], &[16, 20]);
    assert!((m.total - (m.startup + m.active + m.recovery)).abs() < 1e-6);
    assert_eq!(unsafe { ks_kungfu_move(84, &mut m) }, KsStatus::KS_OK);
    assert_eq!((string(m.key).as_str(), m.style, m.kind, m.guard), ("ToadSpecial", 6, KS_MOVE_SPECIAL, KS_GUARD_UNBLOCKABLE));
    assert_eq!(unsafe { ks_kungfu_move(85, &mut m) }, KsStatus::KS_ERR_NOT_FOUND);
    assert_eq!(unsafe { ks_kungfu_move(u32::MAX, &mut m) }, KsStatus::KS_ERR_NOT_FOUND);
    assert_eq!(unsafe { ks_kungfu_move(0, null_mut()) }, KsStatus::KS_ERR_NULL);
    for id in 0..85 {
        assert_eq!(unsafe { ks_kungfu_move(id, &mut m) }, KsStatus::KS_OK);
        assert_eq!(m.id, id);
        assert!(!string(m.name).is_empty() && !string(m.key).is_empty());
    }
    assert_eq!(ks_kungfu_style_count(), 7);
    assert_eq!(string(ks_kungfu_style_name(6)), "Toad");
    assert_eq!(ks_kungfu_style_name(7).len, 0);
    assert_eq!(string(ks_prop_kind_name(0)), "bowl");
    assert_eq!(ks_prop_kind_name(999).len, 0);
    assert_eq!(string(ks_weapon_name(6)), "syrup");
    assert_eq!(string(ks_stage_name(KS_STAGE_FREE_FOR_ALL)), "FreeForAll");
    let r = ks_kungfu_rules();
    assert_eq!((r.styles, r.moves, r.kos), (7, 85, 10));
    // Every sound the events name is a file the game ships.
    for sound in 1..KS_SOUND_COUNT {
        let file = string(ks_sound_file(sound));
        assert!(std::path::Path::new(&format!("{}/../../assets/{file}", env!("CARGO_MANIFEST_DIR"))).is_file(), "{sound}: {file}");
    }
    assert_eq!(ks_sound_file(0).len, 0);
    assert_eq!(ks_sound_volume_at(0.), 1.);
    assert_eq!(ks_sound_volume_at(f32::NAN), 0.2);
}

#[test]
fn null_destroyed_and_made_up_handles_are_refused_not_followed() {
    let mut word = 0u64;
    let made_up = &mut word as *mut u64 as *mut KsGame;
    let destroyed = create("", "", 1, 0);
    unsafe { ks_game_destroy(destroyed) };
    let mut m = unsafe { std::mem::zeroed::<KsMatch>() };
    let mut p = [unsafe { std::mem::zeroed::<KsPlayer>() }; 4];
    let mut n = 0;
    let c = command([0., 0.], 0., 0);
    for (handle, expected) in [(null_mut(), KsStatus::KS_ERR_NULL), (made_up, KsStatus::KS_ERR_INVALID_ARGUMENT), (destroyed, KsStatus::KS_ERR_INVALID_ARGUMENT), (std::ptr::dangling_mut::<KsGame>(), KsStatus::KS_ERR_INVALID_ARGUMENT)] {
        unsafe {
            assert_eq!(ks_game_match(handle, &mut m), expected);
            assert_eq!(ks_game_players(handle, p.as_mut_ptr(), 4, &mut n), expected);
            assert_eq!(ks_game_set_command(handle, &c), expected);
            assert_eq!(ks_game_step(handle, 0.016), expected);
            assert_eq!(ks_game_restart(handle), expected);
            assert_eq!(ks_client_receive_binary(handle, [2u8, 1, 0].as_ptr(), 3), expected);
            assert_eq!(ks_client_receive_text(handle, b"{}".as_ptr(), 2), expected);
            let mut msg = KsMessage { kind: 9, data: KsStr::EMPTY };
            assert_eq!(ks_client_poll(handle, &mut msg), expected);
            assert_eq!(ks_game_events(handle, null_mut(), 0, &mut n), expected);
            let mut info = std::mem::zeroed::<KsMapInfo>();
            assert_eq!(ks_map_info(handle, &mut info), expected);
            assert_eq!(ks_game_local_player(handle), 0);
            assert_eq!(ks_game_event_count(handle), 0);
            assert_eq!(ks_game_is_online(handle), 0);
        }
        assert!(!error().is_empty());
    }
    // Destroying nothing, or twice, is passed over.
    unsafe {
        ks_game_destroy(null_mut());
        ks_game_destroy(destroyed);
        ks_game_destroy(made_up);
    }
    assert_eq!(word, 0, "a made-up handle is never written through");
}

#[test]
fn garbage_inputs_are_errors() {
    let mut g = std::ptr::dangling_mut::<KsGame>();
    unsafe {
        assert_eq!(ks_game_create(s("not json"), s(""), 0, 0, &mut g), KsStatus::KS_ERR_MAP);
        assert!(g.is_null(), "a failed create leaves no handle");
        assert!(error().contains("ks_game_create"));
        assert_eq!(ks_game_create(s(r#"{"format":"hotdog-map","bounds":"wide"}"#), s(""), 0, 0, &mut g), KsStatus::KS_ERR_MAP);
        let bad = [0xff, 0xfe, b'{'];
        assert_eq!(ks_game_create(KsStr { ptr: bad.as_ptr(), len: 3 }, s(""), 0, 0, &mut g), KsStatus::KS_ERR_UTF8);
        assert_eq!(ks_game_create(KsStr { ptr: null(), len: 5 }, s(""), 0, 0, &mut g), KsStatus::KS_ERR_NULL);
        assert_eq!(ks_game_create(s(""), KsStr { ptr: bad.as_ptr(), len: 2 }, 0, 0, &mut g), KsStatus::KS_ERR_UTF8);
        assert_eq!(ks_game_create(s(""), s(""), 0, 0x80, &mut g), KsStatus::KS_ERR_INVALID_ARGUMENT);
        assert_eq!(ks_game_create(s(""), s(""), 0, 0, null_mut()), KsStatus::KS_ERR_NULL);
        // Rules it does not know are passed over, as the server's are.
        let g = create("", "mode=nonsense weapons=everything", 0, 0);
        assert_eq!(matched(g).mode, KS_MODE_SIEGE);
        for bad in [
            KsCommand { stick_x: f32::NAN, ..command([0., 0.], 0., 0) },
            KsCommand { yaw: f32::INFINITY, ..command([0., 0.], 0., 0) },
            KsCommand { weapon: 8, ..command([0., 0.], 0., 0) },
            KsCommand { pickup: -2, ..command([0., 0.], 0., 0) },
            KsCommand { pickup: 9999, ..command([0., 0.], 0., 0) },
        ] {
            assert_eq!(ks_game_set_command(g, &bad), KsStatus::KS_ERR_INVALID_ARGUMENT, "{bad:?}");
        }
        assert_eq!(ks_game_set_command(g, null()), KsStatus::KS_ERR_NULL);
        // Buttons past the last are masked, not refused.
        assert_eq!(ks_game_set_command(g, &command([3., -7.], 100., !KS_BUTTON_FIRE)), KsStatus::KS_OK);
        for dt in [f32::NAN, -0.1, 10.5, f32::INFINITY] {
            assert_eq!(ks_game_step(g, dt), KsStatus::KS_ERR_INVALID_ARGUMENT, "{dt}");
        }
        assert_eq!(ks_game_step(g, 0.), KsStatus::KS_OK);
        let mut n = 0;
        assert_eq!(ks_game_players(g, null_mut(), 3, &mut n), KsStatus::KS_ERR_NULL);
        assert_eq!(ks_game_players(g, null_mut(), 0, null_mut()), KsStatus::KS_ERR_NULL);
        assert_eq!(ks_game_events(g, null_mut(), 1, &mut n), KsStatus::KS_ERR_NULL);
        assert_eq!(ks_game_player(g, 77, &mut std::mem::zeroed()), KsStatus::KS_ERR_NOT_FOUND);
        assert_eq!(ks_cards_command(g, 9, 0), KsStatus::KS_ERR_INVALID_ARGUMENT);
        assert_eq!(ks_cards_command(g, KS_CARDS_FLIP, 0), KsStatus::KS_ERR_REFUSED, "no table in a siege");
        assert_eq!(ks_game_set_bot(g, 42, 1), KsStatus::KS_ERR_NOT_FOUND);
        let mut y = 0.;
        assert_eq!(ks_game_ground(g, f32::NAN, 0., &mut y), KsStatus::KS_ERR_INVALID_ARGUMENT);
        assert_eq!(ks_game_ground(g, 0., 2.4, &mut y), KsStatus::KS_OK);
        assert!((y - kitchen_sim::feast::FLOOR).abs() < 0.3);
        // The other mode's calls.
        let mut msg = KsMessage { kind: 0, data: KsStr::EMPTY };
        assert_eq!(ks_client_poll(g, &mut msg), KsStatus::KS_ERR_WRONG_MODE);
        assert_eq!(ks_client_receive_text(g, b"{}".as_ptr(), 2), KsStatus::KS_ERR_WRONG_MODE);
        assert_eq!(ks_client_set_prediction(g, 0), KsStatus::KS_ERR_WRONG_MODE);
        assert_eq!(ks_client_status(g, &mut std::mem::zeroed()), KsStatus::KS_ERR_WRONG_MODE);
        assert_eq!(ks_kungfu_state(g, &mut std::mem::zeroed()), KsStatus::KS_ERR_NOT_FOUND, "a siege is not Kung Fu");
        assert_eq!(ks_game_forts(g, &mut std::mem::zeroed()), KsStatus::KS_ERR_NOT_FOUND);
        let mut text = KsStr::EMPTY;
        assert_eq!(ks_cards_json(g, &mut text), KsStatus::KS_ERR_NOT_FOUND);
        assert_eq!(ks_map_json(g, &mut text), KsStatus::KS_OK);
        assert!(Map::parse(&string(text)).is_ok(), "the map's text reads back");
        ks_game_destroy(g);
    }
}

#[test]
fn an_offline_free_for_all_plays_and_reports() {
    let g = create(&read("assets/maps/counter-siege.json"), "mode=ffa", 7, 0);
    let m = matched(g);
    assert_eq!((m.mode, m.stage, m.goal, m.local_player, m.players), (KS_MODE_FFA, KS_STAGE_FREE_FOR_ALL, 25, 1, 5));
    assert_eq!(string(m.rules), "mode=ffa");
    assert!(m.time_left > 299.);
    let me = players(g)[0];
    assert!(me.flags & KS_PLAYER_LOCAL != 0 && me.flags & KS_PLAYER_BOT == 0 && me.flags & KS_PLAYER_ALIVE != 0);
    assert_eq!(string(me.name), "BUNNER");
    let start = me.p;
    // The local hotdog walks ahead (its yaw is where it looks).
    for _ in 0..30 {
        step(g, &command([0., -1.], me.yaw, 0), 1. / 60.);
    }
    let walked = players(g)[0].p;
    let far = ((walked[0] - start[0]).powi(2) + (walked[2] - start[2]).powi(2)).sqrt();
    assert!(far > 0.05, "walked {far} m");
    let mut all = vec![];
    for k in 0..900 {
        // A jump now and then (an edge: set once, sent once).
        step(g, &command([0.3, -1.], me.yaw + k as f32 * 0.01, if k % 120 == 0 { KS_BUTTON_JUMP } else { 0 }), 1. / 60.);
        all.extend(events(g));
    }
    let kinds = |k: u32| all.iter().filter(|e| e.kind == k).count();
    assert!(kinds(KS_EVENT_SHOT) > 0, "the bots fire: {:?}", all.iter().map(|e| e.kind).collect::<Vec<_>>());
    assert!(kinds(KS_EVENT_DAMAGE) > 0, "and hit");
    let m = matched(g);
    assert!((m.elapsed - 15.5).abs() < 0.1, "{}", m.elapsed);
    assert!(m.time_left < 300. - 15.);
    // Everything else reads.
    let mut info = unsafe { std::mem::zeroed::<KsMapInfo>() };
    unsafe {
        assert_eq!(ks_map_info(g, &mut info), KsStatus::KS_OK);
        assert_eq!(string(info.name), "Counter Siege");
        let mut n = 0;
        assert_eq!(ks_map_blocks(g, null_mut(), 0, &mut n), KsStatus::KS_OK);
        assert_eq!(n, info.blocks);
        let mut blocks = vec![std::mem::zeroed::<KsBlock>(); n as usize];
        assert_eq!(ks_map_blocks(g, blocks.as_mut_ptr(), n, &mut n), KsStatus::KS_OK);
        assert!(blocks.iter().all(|b| b.rect[0] <= b.rect[2] && b.rect[1] <= b.rect[3] && !string(b.style).is_empty()));
        let mut props = vec![std::mem::zeroed::<KsProp>(); 64];
        assert_eq!(ks_game_props(g, props.as_mut_ptr(), 64, &mut n), KsStatus::KS_OK);
        for p in &props[..n.min(64) as usize] {
            let q = p.q;
            assert!(((q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt() - 1.).abs() < 1e-3);
            assert!(p.kind < ks_prop_kind_count());
        }
        let mut few = [std::mem::zeroed::<KsPlayer>(); 2];
        assert_eq!(ks_game_players(g, few.as_mut_ptr(), 2, &mut n), KsStatus::KS_OK);
        assert_eq!(n, 5, "count is all of them, however few fit");
        assert_eq!(few[1].id, 2);
        let mut ground = 0.;
        assert_eq!(ks_game_ground(g, walked[0], walked[2], &mut ground), KsStatus::KS_OK);
        assert_eq!(ks_map_decks(g, null_mut(), 0, &mut n), KsStatus::KS_OK);
        assert_eq!(n, info.decks);
        assert_eq!(ks_map_ramps(g, null_mut(), 0, &mut n), KsStatus::KS_OK);
        assert_eq!(n, info.ramps);
        assert_eq!(ks_map_spawns(g, null_mut(), 0, &mut n), KsStatus::KS_OK);
        assert_eq!(n, info.spawns);
        assert_eq!(ks_game_pickups(g, null_mut(), 0, &mut n), KsStatus::KS_OK);
        assert_eq!(ks_game_turrets(g, null_mut(), 0, &mut n), KsStatus::KS_OK);
        ks_game_destroy(g);
    }
}

#[test]
fn a_kung_fu_bot_match_fights_and_tells_its_events() {
    let g = create(&read("assets/maps/kung-fu-dojo.json"), "", 3, KS_GAME_ALL_BOTS);
    let m = matched(g);
    assert_eq!((m.mode, m.has_kungfu, m.goal), (KS_MODE_KUNGFU, 1, 10));
    let mut all = vec![];
    for _ in 0..600 {
        // (Ignored: everyone is a bot.)
        step(g, &command([0., -1.], 0., KS_BUTTON_FIRE), 1. / 60.);
        all.extend(events(g));
    }
    let mut k = unsafe { std::mem::zeroed::<KsKungfu>() };
    unsafe { assert_eq!(ks_kungfu_state(g, &mut k), KsStatus::KS_OK) };
    assert_eq!((k.fighters, k.goal), (5, 10));
    let mut fighters = [KsFighter::default(); 8];
    let mut n = 0;
    unsafe { assert_eq!(ks_kungfu_fighters(g, fighters.as_mut_ptr(), 8, &mut n), KsStatus::KS_OK) };
    for f in &fighters[..n as usize] {
        assert!(f.mv < 85 && f.style < 7 && (0. ..=100.).contains(&f.chi) && f.t >= 0., "{f:?}");
        assert!((f.t - (k.clock - f.started).max(0.)).abs() < 1e-4);
    }
    let fu: Vec<&KsEvent> = all.iter().filter(|e| (KS_EVENT_FU_HIT..=KS_EVENT_FU_SPECIAL).contains(&e.kind)).collect();
    assert!(all.iter().any(|e| e.kind == KS_EVENT_FU_MOVE && e.sound != 0), "moves whoosh");
    assert!(fu.iter().any(|e| e.kind == KS_EVENT_FU_HIT && e.sound != 0 && e.to != 0), "blows land: {:?}", fu.iter().map(|e| e.kind).collect::<Vec<_>>());
    // Each of the fight's events is told once, in order.
    let ids: Vec<u32> = fu.iter().map(|e| e.id).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    // Nobody holds a gun or a pickup in Kung Fu; the styles are the "weapons".
    assert!(players(g).iter().all(|p| (1..=7).contains(&p.weapon)));
    unsafe { ks_game_destroy(g) };
}

#[test]
fn the_same_seed_and_commands_play_the_same_game() {
    let run = |seed: u64| {
        let g = create(&read("assets/maps/kung-fu-dojo.json"), "", seed, 0);
        for k in 0..300 {
            let buttons = if k % 40 == 0 { KS_BUTTON_FIRE } else { 0 } | if k % 90 == 5 { KS_BUTTON_STIM } else { 0 };
            step(g, &command([(k as f32 * 0.05).sin(), -0.5], k as f32 * 0.02, buttons), 1. / 60.);
        }
        let out: Vec<([f32; 3], f32)> = players(g).iter().map(|p| (p.p, p.hp)).collect();
        unsafe { ks_game_destroy(g) };
        out
    };
    assert_eq!(run(11), run(11));
}

#[test]
fn edges_go_once_and_held_buttons_stay() {
    let g = create(&read("assets/maps/counter-siege.json"), "sandbox botmode=idle", 1, 0);
    let me = players(g)[0];
    // A jump pressed once: one jump, however many steps follow without setting the command again.
    step(g, &command([0., 0.], me.yaw, KS_BUTTON_JUMP), 1. / 60.);
    let mut highest = 0f32;
    for _ in 0..120 {
        unsafe { assert_eq!(ks_game_step(g, 1. / 60.), KsStatus::KS_OK) };
        highest = highest.max(players(g)[0].p[1] - me.p[1]);
    }
    assert!(highest > 0.05, "it jumped ({highest})");
    let landed = players(g)[0].p[1];
    for _ in 0..60 {
        unsafe { assert_eq!(ks_game_step(g, 1. / 60.), KsStatus::KS_OK) };
    }
    assert!((players(g)[0].p[1] - landed).abs() < 1e-3, "and only once");
    // Crouch is held: it stays crouched until the command says otherwise.
    step(g, &command([0., 0.], me.yaw, KS_BUTTON_CROUCH), 1. / 60.);
    unsafe { assert_eq!(ks_game_step(g, 1. / 60.), KsStatus::KS_OK) };
    assert_eq!(players(g)[0].stance, KS_STANCE_CROUCH);
    step(g, &command([0., 0.], me.yaw, 0), 1. / 60.);
    assert_eq!(players(g)[0].stance, KS_STANCE_STAND);
    unsafe { ks_game_destroy(g) };
}

// ----- Online

/// The server's side, as crates/server's hub encodes for one client: the round's frame, made into whole numbers, against
/// the frame the client acknowledged, with the events it has not been told and its own body.
struct Server {
    feast: Feast,
    tick: u32,
    applied: u32,
    kept: Vec<(u32, Wire)>,
    told: [u32; 3],
}
impl Server {
    fn new(map: Map) -> Server {
        let mut feast = Feast::with_map(map, false);
        feast.follow = true;
        Server { feast, tick: 0, applied: 0, kept: vec![], told: [0; 3] }
    }
    fn frame(&self) -> Frame {
        let f = &self.feast;
        Frame {
            kind: "snapshot".into(),
            round: f.round,
            elapsed: f.elapsed,
            stage: kitchen_sim::net::STAGES[stage_index(f.stage) as usize].into(),
            warmup: f.warmup,
            famine_at: f.famine_at,
            alert: f.alert.clone(),
            winner: f.winner,
            wave_spawned: f.wave_spawned,
            wave_kills: f.wave_kills,
            friendly_fire_events: f.friendly_fire_events,
            players: f.players.iter().map(|p| Seat { player: p.clone(), color: if p.id == 1 { "blue".into() } else { String::new() }, human: p.id == 1, ..Default::default() }).collect(),
            enemies: f.enemies.clone(),
            emergences: f.emergences.clone(),
            pickups: f.pickups.clone(),
            shots: f.shots.clone(),
            projectiles: f.projectiles.clone(),
            splats: f.splats.clone(),
            pests: f.pests.clone(),
            turrets: f.turret_yaw.clone(),
            props: (0..f.props.len()).map(|id| Loose { id, state: f.props.state(id) as u8, p: f.props.position(id), q: f.props.rotation(id), v: f.props.velocity(id), w: f.props.angular_velocity(id) }).collect(),
            knocks: f.knocks.clone(),
            drapes: f.drapes.clone(),
            forts: f.fort_state(),
            rules: f.rules.text(),
            ends: f.ends,
            ..Default::default()
        }
    }
    /// A frame for the client that acknowledged `acked` (0: whole).
    fn send(&mut self, acked: u32) -> Vec<u8> {
        let frame = self.frame();
        let state = frame.state();
        let base = self.kept.iter().find(|(t, _)| *t == acked && acked != 0);
        let events = Events {
            shots: frame.shots.iter().filter(|s| s.id > self.told[0]).cloned().collect(),
            splats: frame.splats.iter().filter(|s| s.id > self.told[1]).cloned().collect(),
            knocks: frame.knocks.iter().filter(|k| k.id > self.told[2]).cloned().collect(),
        };
        self.told = [
            events.shots.iter().map(|s| s.id).fold(self.told[0], u32::max),
            events.splats.iter().map(|s| s.id).fold(self.told[1], u32::max),
            events.knocks.iter().map(|k| k.id).fold(self.told[2], u32::max),
        ];
        let mine = self.feast.player(1).filter(|p| p.alive).and(self.feast.body(1)).map(Mine::of);
        let header = Header { tick: self.tick, base: base.map_or(0, |b| b.0), ack: self.applied, checksum: state.checksum(), elapsed: frame.elapsed, mine };
        let bytes = kitchen_sim::net::encode_frame(&header, base.map(|b| &b.1), &state, &events);
        self.kept.push((self.tick, state));
        if self.kept.len() > 64 {
            self.kept.remove(0);
        }
        bytes
    }
}
fn poll_all(g: *mut KsGame) -> Vec<(u32, Vec<u8>)> {
    let mut out = vec![];
    loop {
        let mut m = KsMessage { kind: 0, data: KsStr::EMPTY };
        assert_eq!(unsafe { ks_client_poll(g, &mut m) }, KsStatus::KS_OK);
        if m.kind == KS_MESSAGE_NONE {
            return out;
        }
        out.push((m.kind, unsafe { std::slice::from_raw_parts(m.data.ptr, m.data.len) }.to_vec()));
    }
}
fn text_in(g: *mut KsGame, t: &str) -> KsStatus {
    unsafe { ks_client_receive_text(g, t.as_ptr(), t.len()) }
}
fn status(g: *const KsGame) -> KsClientStatus {
    let mut st = unsafe { std::mem::zeroed::<KsClientStatus>() };
    assert_eq!(unsafe { ks_client_status(g, &mut st) }, KsStatus::KS_OK);
    st
}

#[test]
fn an_online_client_joins_decodes_frames_sends_commands_and_predicts() {
    let map = Map::parse(&read("assets/maps/counter-siege.json")).unwrap();
    let mut server = Server::new(map.clone());
    let join = KsJoin { name: s("UNREAL"), color: s("blue"), team: KsStr::EMPTY, token: KsStr::EMPTY, sandbox: KsStr::EMPTY, json: 0 };
    let mut g = null_mut();
    unsafe { assert_eq!(ks_client_create(&join, &mut g), KsStatus::KS_OK) };
    assert_eq!(unsafe { ks_game_is_online(g) }, 1);
    // The join goes first.
    let sent = poll_all(g);
    assert_eq!(sent.len(), 1);
    let v: serde_json::Value = serde_json::from_slice(&sent[0].1).unwrap();
    assert_eq!((sent[0].0, v["type"].as_str(), v["version"].as_u64(), v["name"].as_str()), (KS_MESSAGE_TEXT, Some("join"), Some(12), Some("UNREAL")));
    // Nothing to read before the welcome; a frame before it is refused.
    let mut n = 0;
    unsafe {
        assert_eq!(ks_game_players(g, null_mut(), 0, &mut n), KsStatus::KS_ERR_NOT_READY);
        assert_eq!(ks_game_match(g, &mut std::mem::zeroed()), KsStatus::KS_ERR_NOT_READY);
        let early = server.send(0);
        assert_eq!(ks_client_receive_binary(g, early.as_ptr(), early.len()), KsStatus::KS_ERR_NOT_READY);
        assert_eq!(ks_game_restart(g), KsStatus::KS_ERR_WRONG_MODE);
        assert_eq!(ks_game_set_bot(g, 2, 1), KsStatus::KS_ERR_WRONG_MODE);
    }
    server.kept.clear();
    let welcome = serde_json::json!({"type": "welcome", "id": 1, "version": 12, "map": map.to_text(), "tick": 60, "send": 30}).to_string();
    assert_eq!(text_in(g, &welcome), KsStatus::KS_OK, "{}", error());
    assert_eq!((status(g).welcomed, status(g).local_player, unsafe { ks_game_local_player(g) }), (1, 1, 1));
    assert_eq!(matched(g).map_generation, 2);
    // The person's hotdog is player 1 on the server; the rest are bots.
    server.feast.players[0].bot = false;
    let me = server.feast.players[0].clone();
    let dt = 1. / 60.;
    let mut acked = 0;
    let mut sent_commands = 0;
    // (As far as three seconds took at the walk the hotdogs once had: at today's it would cross the counter into the bots.)
    let walk = (180. / kitchen_sim::actor::FASTER).round() as usize;
    for k in 0..240 {
        // The client: its input this frame, a step, what it sends.
        let stick = if k < walk { [0.4, -1.] } else { [0., 0.] };
        step(g, &command(stick, me.yaw, if k == 30 { KS_BUTTON_JUMP } else { 0 }), dt);
        for (kind, bytes) in poll_all(g) {
            assert_eq!(kind, KS_MESSAGE_BINARY);
            let batch = Batch::decode(&bytes).expect("a batch the server reads");
            acked = batch.ack;
            // The server: each command not had yet, applied as it applies a person's, then its step.
            let had = server.applied;
            for c in batch.commands.iter().filter(|c| c.seq > had) {
                session::apply_command(&mut server.feast, 1, c, dt);
                server.applied = c.seq;
                sent_commands += 1;
            }
        }
        server.feast.tick(dt);
        server.tick += 1;
        if k % 2 == 1 {
            let bytes = server.send(acked);
            assert_eq!(unsafe { ks_client_receive_binary(g, bytes.as_ptr(), bytes.len()) }, KsStatus::KS_OK, "frame {}: {}", server.tick, error());
        }
    }
    assert_eq!(sent_commands, 240, "one command a step, each applied once");
    let st = status(g);
    assert_eq!((st.whole, st.broken), (1, 0), "one whole frame, then deltas against what the client acknowledged");
    assert_eq!(st.frames, 120);
    assert_eq!(st.newest_tick, server.tick);
    // The others are where the server has them (to the millimetre the frames carry); this client's own hotdog, predicted
    // and then at rest, where the server's is.
    let theirs: Vec<KsPlayer> = players(g);
    for (p, q) in theirs.iter().zip(&server.feast.players) {
        assert_eq!(p.id, q.id);
        let far = (0..3).map(|i| (p.p[i] - q.p[i]).powi(2)).sum::<f32>().sqrt();
        assert!(far < 2e-3, "hotdog {}: {:?} vs {:?}", p.id, p.p, q.p);
    }
    let mine = theirs[0];
    assert!(mine.flags & KS_PLAYER_LOCAL != 0 && mine.flags & KS_PLAYER_HUMAN != 0 && mine.flags & KS_PLAYER_PREDICTED != 0, "{:#x}", mine.flags);
    assert_eq!(string(mine.color), "blue");
    let moved = ((mine.p[0] - me.p[0]).powi(2) + (mine.p[2] - me.p[2]).powi(2)).sqrt();
    assert!(moved > 0.3, "the hotdog walked {moved} m");
    assert!(st.predicting == 1 && st.corrections < 10, "{} corrections", st.corrections);
    // Props and the round's state came across.
    let m = matched(g);
    assert_eq!((m.stage, m.round, m.local_player), (stage_index(server.feast.stage), server.feast.round, 1));
    assert!((m.elapsed - server.feast.elapsed).abs() < 2e-3);
    let mut props = vec![KsProp::default(); 64];
    unsafe { assert_eq!(ks_game_props(g, props.as_mut_ptr(), 64, &mut n), KsStatus::KS_OK) };
    assert_eq!(n as usize, server.feast.props.len());
    for p in &props[..n as usize] {
        let q = server.feast.props.position(p.id as usize);
        assert!((0..3).all(|i| (p.p[i] as f64 - q[i]).abs() < 1e-3), "prop {}", p.id);
    }
    unsafe { ks_game_destroy(g) };
}

#[test]
fn a_broken_frame_asks_for_a_whole_one_and_text_messages_are_taken() {
    let map = Map::parse(&read("assets/maps/kung-fu-dojo.json")).unwrap();
    let mut server = Server::new(map.clone());
    let mut g = null_mut();
    unsafe { assert_eq!(ks_client_create(null(), &mut g), KsStatus::KS_OK) };
    poll_all(g);
    let welcome = serde_json::json!({"type": "welcome", "id": 2, "map": map.to_text(), "tick": 60, "send": 30}).to_string();
    assert_eq!(text_in(g, &welcome), KsStatus::KS_OK);
    server.tick = 1;
    let first = server.send(0);
    unsafe { assert_eq!(ks_client_receive_binary(g, first.as_ptr(), first.len()), KsStatus::KS_OK, "{}", error()) };
    // A delta against the first: fine.
    server.feast.tick(0.1);
    server.tick += 6;
    let delta = server.send(1);
    unsafe { assert_eq!(ks_client_receive_binary(g, delta.as_ptr(), delta.len()), KsStatus::KS_OK, "{}", error()) };
    // A frame lost on the way, and the next against it: the client has not got its base.
    server.feast.tick(0.1);
    server.tick += 6;
    let _lost = server.send(7);
    server.feast.tick(0.1);
    server.tick += 6;
    let orphan = server.send(13);
    unsafe { assert_eq!(ks_client_receive_binary(g, orphan.as_ptr(), orphan.len()), KsStatus::KS_ERR_MALFORMED) };
    assert!(error().contains("has not got"), "{}", error());
    // A whole frame corrupted on the way: an error, or (a byte the state does not cover) taken.
    server.feast.tick(0.1);
    server.tick += 6;
    let mut corrupt = server.send(0);
    let at = corrupt.len() / 2;
    corrupt[at] ^= 0x5a;
    let status_of = unsafe { ks_client_receive_binary(g, corrupt.as_ptr(), corrupt.len()) };
    assert!(matches!(status_of, KsStatus::KS_ERR_MALFORMED | KsStatus::KS_OK), "{status_of:?}");
    unsafe { assert_eq!(ks_game_step(g, 1. / 60.), KsStatus::KS_OK) };
    let batches: Vec<Batch> = poll_all(g).iter().map(|(_, b)| Batch::decode(b).unwrap()).collect();
    assert!(batches.iter().any(|b| b.resync), "a whole frame is asked for");
    assert!(status(g).broken >= 1);
    // Kung Fu's view, the griddles, the cards and the tuning come as text; garbage is an error, an unknown type is not.
    server.feast.tick(1.);
    let view = server.feast.kungfu_view().unwrap();
    let mut told = serde_json::to_value(&view).unwrap();
    told["type"] = "kungfu".into();
    assert_eq!(text_in(g, &told.to_string()), KsStatus::KS_OK);
    let mut k = KsKungfu::default();
    unsafe { assert_eq!(ks_kungfu_state(g, &mut k), KsStatus::KS_OK) };
    assert_eq!(k.fighters as usize, view.fighters.len());
    let mut fighters = [KsFighter::default(); 8];
    let mut n = 0;
    unsafe { assert_eq!(ks_kungfu_fighters(g, fighters.as_mut_ptr(), 8, &mut n), KsStatus::KS_OK) };
    assert_eq!(fighters[0].style as u8, view.fighters[0].style);
    assert_eq!(text_in(g, r#"{"type":"griddle","griddles":[{"block":0,"on":true,"temp":150.0,"by":2}]}"#), KsStatus::KS_OK);
    assert_eq!(text_in(g, r#"{"type":"tuning","text":"","editable":true}"#), KsStatus::KS_OK);
    assert_eq!((status(g).tuning_editable, status(g).tuning.len), (1, 0));
    assert_eq!(text_in(g, r#"{"type":"cards","table":{"nonsense":1}}"#), KsStatus::KS_OK, "an unreadable table is passed over");
    assert_eq!(text_in(g, r#"{"type":"error","message":"Kitchen full (5 hotdogs)"}"#), KsStatus::KS_OK);
    assert_eq!(string(status(g).error), "Kitchen full (5 hotdogs)");
    assert_eq!(text_in(g, r#"{"type":"something new"}"#), KsStatus::KS_OK);
    assert_eq!(text_in(g, "[1, 2]"), KsStatus::KS_ERR_MALFORMED);
    assert_eq!(text_in(g, "{not json"), KsStatus::KS_ERR_MALFORMED);
    assert_eq!(text_in(g, r#"{"type":"map","id":3,"map":"not a map"}"#), KsStatus::KS_ERR_MALFORMED, "a map that does not read");
    assert_eq!(unsafe { ks_client_receive_text(g, [0xc3u8, 0x28].as_ptr(), 2) }, KsStatus::KS_ERR_UTF8);
    // The local player's wishes go as text.
    unsafe {
        assert_eq!(ks_game_ready(g, 1), KsStatus::KS_OK);
        assert_eq!(ks_game_use_griddle(g), KsStatus::KS_OK);
        assert_eq!(ks_cards_command(g, KS_CARDS_TARGET, 3), KsStatus::KS_OK);
        assert_eq!(ks_client_send_text(g, s(r#"{"type":"talk","level":0.5}"#)), KsStatus::KS_OK);
        assert_eq!(ks_client_send_text(g, s("nope")), KsStatus::KS_ERR_INVALID_ARGUMENT);
    }
    let texts: Vec<serde_json::Value> = poll_all(g).iter().filter(|(k, _)| *k == KS_MESSAGE_TEXT).map(|(_, b)| serde_json::from_slice(b).unwrap()).collect();
    assert_eq!(texts.len(), 4);
    assert_eq!(texts[0], serde_json::json!({"type": "ready", "ready": true}));
    assert_eq!(texts[1], serde_json::json!({"type": "griddle"}));
    assert_eq!(texts[2], serde_json::json!({"type": "cards", "action": "target", "seat": 3}));
    // A new map: a new game, read again.
    let other = Map::parse(&read("assets/maps/counter-siege.json")).unwrap();
    let before = matched(g).map_generation;
    assert_eq!(text_in(g, &serde_json::json!({"type": "map", "id": 4, "map": other.to_text()}).to_string()), KsStatus::KS_OK);
    assert_eq!((matched(g).map_generation, unsafe { ks_game_local_player(g) }), (before + 1, 4));
    assert!(events(g).iter().any(|e| e.kind == KS_EVENT_MAP));
    // The debug encoding: a whole frame as JSON.
    let mut siege = Server::new(other);
    siege.feast.tick(0.5);
    let mut snap = serde_json::to_value(siege.frame()).unwrap();
    snap["tick"] = 5.into();
    assert_eq!(text_in(g, &snap.to_string()), KsStatus::KS_OK, "{}", error());
    assert_eq!(players(g).len(), 5);
    unsafe { ks_game_destroy(g) };
}

#[test]
fn fuzzed_messages_never_panic() {
    let map = Map::parse(&read("assets/maps/counter-siege.json")).unwrap();
    let mut server = Server::new(map.clone());
    let mut g = null_mut();
    unsafe { assert_eq!(ks_client_create(null(), &mut g), KsStatus::KS_OK) };
    assert_eq!(text_in(g, &serde_json::json!({"type": "welcome", "id": 1, "map": map.to_text()}).to_string()), KsStatus::KS_OK);
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut statuses = std::collections::BTreeMap::new();
    for round in 0..3000 {
        // A good frame newer than any before, to be cut short or changed.
        server.tick += 1;
        let good = server.send(0);
        let mut bytes: Vec<u8> = match round % 3 {
            // Random bytes, starting as a frame does or not.
            0 => (0..(next() % 64)).map(|_| next() as u8).collect(),
            // A good frame cut short or with a byte changed.
            1 => good[..(next() as usize % good.len())].to_vec(),
            _ => {
                let mut b = good.clone();
                let i = next() as usize % b.len();
                b[i] = next() as u8;
                b
            }
        };
        if round % 2 == 0 && !bytes.is_empty() {
            bytes[0] = 2;
        }
        let st = unsafe { ks_client_receive_binary(g, bytes.as_ptr(), bytes.len()) };
        assert_ne!(st, KsStatus::KS_ERR_PANIC, "{}", error());
        *statuses.entry(format!("{st:?}")).or_insert(0) += 1;
        const ALPHABET: &[u8] = b" {}[]\":,typewlcomapsnhkgfu0123456789";
        let text: String = (0..(next() % 40)).map(|_| char::from(ALPHABET[next() as usize % ALPHABET.len()])).collect();
        assert_ne!(text_in(g, &text), KsStatus::KS_ERR_PANIC, "{text}: {}", error());
        unsafe { assert_eq!(ks_game_step(g, 1. / 60.), KsStatus::KS_OK) };
        poll_all(g);
    }
    assert!(statuses.contains_key("KS_ERR_MALFORMED"), "{statuses:?}");
    // It still reads.
    assert_eq!(players(g).len(), 5);
    unsafe { ks_game_destroy(g) };
}

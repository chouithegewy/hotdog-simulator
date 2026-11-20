// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The replay format: what goes in comes out, a file cut short plays to where it ends, another format is refused,
//! a seek plays exactly what playing from the start plays, and the stats count what happened.
use super::*;
use crate::feast::{Damage, Player, Shot};
use crate::map::Map;
use crate::rules::Rules;

const STEP: f32 = 1. / 60.;

/// A game of bots played for `seconds` and recorded as a client would be sent it: a frame every two steps, and the
/// reserves' text whenever they change. Returns the file and each frame's time and state as it was made.
fn record(seconds: f32, rules: &str) -> (Vec<u8>, Vec<(u32, State)>, Vec<(u32, String)>) {
    let map = Map::bundled();
    let mut feast = Feast::with_map(map.clone(), true);
    if !rules.is_empty() {
        feast.set_rules(Rules::parse(rules));
    }
    let first = snapshot(&feast, None, |_| "red".into());
    let header = Header::new("kitchen-sim test", "offline", &map, "siege", rules, roster(&first), 1_700_000_000);
    let mut w = Writer::new(Vec::new(), &header).unwrap();
    let (mut states, mut texts) = (vec![], vec![]);
    let mut said = String::new();
    let steps = (seconds / STEP) as u32;
    for tick in 1..=steps {
        feast.tick(STEP);
        if tick % 2 != 0 {
            continue;
        }
        let ms = (tick as f32 * STEP * 1000.).round() as u32;
        let ammo = say::ammo(&feast.stocks());
        if ammo != said {
            w.text(ms, &ammo);
            texts.push((ms, ammo.clone()));
            said = ammo;
        }
        let frame = snapshot(&feast, None, |_| "red".into());
        let state = frame.state();
        w.frame(ms, tick, &frame, &state).unwrap();
        states.push((ms, state));
    }
    let (bytes, summary) = w.finish(1_700_000_100).unwrap();
    assert_eq!(summary.duration_ms, states.last().unwrap().0);
    (bytes, states, texts)
}
/// Plays a whole replay from the start: each frame's time and state, and each text's time and text.
fn play_all(replay: &Replay) -> (Vec<(u32, State)>, Vec<(u32, String)>, Playhead) {
    let (mut head, mut played) = Playhead::seek(replay, 0).unwrap();
    played.extend(head.advance(replay, u32::MAX));
    let start = replay.chunks[0].start;
    let (mut states, mut texts) = (vec![], vec![]);
    let mut decoder = Decoder::default();
    for p in played {
        match p.record {
            Record::Frame(bytes) => {
                decoder.frame(&bytes).unwrap();
                states.push((p.ms, decoder.state().unwrap().clone()));
            }
            // (A text sent before the first frame is in the first chunk's context, which the first seek applies.)
            Record::Text(t) => texts.push((if p.context { start.min(p.ms) } else { p.ms }, t)),
        }
    }
    (states, texts, head)
}

#[test]
fn a_recording_plays_back_every_frame_and_text_exactly_as_it_was_made() {
    let (bytes, states, texts) = record(24., "");
    let replay = Replay::parse(bytes).unwrap();
    assert!(!replay.truncated);
    let h = &replay.header;
    assert_eq!((h.format, h.protocol, h.source.as_str(), h.map_name.as_str()), (FORMAT, net::PROTOCOL, "offline", Map::bundled().name.as_str()));
    assert_eq!(Map::parse(&h.map).unwrap().to_text(), Map::bundled().to_text(), "the whole map travels in the header");
    assert_eq!(h.roster.len(), 5);
    assert!(replay.chunks.len() >= 4, "a keyframe every {KEYFRAME_MS} ms: {} chunks", replay.chunks.len());
    assert_eq!(replay.duration(), states.last().unwrap().0);
    let (played, said, head) = play_all(&replay);
    assert!(head.done(&replay) && head.stopped.is_none());
    assert_eq!(played.len(), states.len());
    for ((ms, a), (mt, b)) in played.iter().zip(&states) {
        assert_eq!(ms, mt);
        assert!(a == b, "the frame at {ms} ms plays as it was made");
    }
    // (The first, sent with the first frame, comes in the first chunk's context, at the moment of that frame.)
    assert_eq!(said, texts, "every text, at its moment");
    // Positions, health and scores sampled from the frames as the client decodes them.
    for (i, (_, state)) in states.iter().enumerate().step_by(97) {
        let made = Frame::from_state(state, 0.);
        let seen = Frame::from_state(&played[i].1, 0.);
        for (a, b) in made.players.iter().zip(&seen.players) {
            assert_eq!((a.player.p, a.player.hp, a.player.score, a.player.kills), (b.player.p, b.player.hp, b.player.score, b.player.kills));
        }
    }
    // The summary says how long and holds the final tally.
    let s = replay.summary.as_ref().unwrap();
    assert_eq!(s.duration_ms, replay.duration());
    assert_eq!(s.lines, head.tally.lines, "the summary's tally is the one a viewer reaches at the end");
}

#[test]
fn a_file_cut_short_plays_up_to_its_last_whole_chunk() {
    let (bytes, states, _) = record(21., "");
    let whole = Replay::parse(bytes.clone()).unwrap();
    for cut in [bytes.len() * 2 / 3, bytes.len() / 2, bytes.len() - 3] {
        let replay = Replay::parse(bytes[..cut].to_vec()).unwrap();
        assert!(replay.truncated && replay.summary.is_none(), "cut at {cut}");
        // (Cut in its last bytes, only the summary is lost.)
        assert!(replay.duration() < whole.duration() || cut == bytes.len() - 3, "cut at {cut}");
        assert!(!replay.chunks.is_empty());
        let (played, _, head) = play_all(&replay);
        assert!(head.stopped.is_none());
        let (ms, last) = played.last().unwrap();
        assert_eq!(*ms, replay.duration());
        let (_, made) = states.iter().find(|(m, _)| m == ms).unwrap();
        assert!(last == made, "the last frame there is plays as it was made");
        // The listing reads it as cut short too, without inflating its chunks.
        let listing = scan(&mut std::io::Cursor::new(&bytes[..cut])).unwrap();
        assert!(listing.truncated && listing.summary.is_none());
        assert_eq!(listing.duration_ms, replay.duration());
    }
    // Cut inside the header: nothing to play.
    assert!(matches!(Replay::parse(bytes[..20].to_vec()), Err(Error::Damaged(_)) | Ok(_)));
    let header_only = Replay::parse(bytes[..20].to_vec());
    assert!(header_only.is_err() || header_only.unwrap().chunks.is_empty());
    assert_eq!(Replay::parse(bytes[..6].to_vec()).err(), Some(Error::NotAReplay));
    // The listing of the whole file.
    let listing = scan(&mut std::io::Cursor::new(&bytes)).unwrap();
    assert!(!listing.truncated);
    assert_eq!((listing.duration_ms, listing.summary.unwrap().duration_ms), (whole.duration(), whole.duration()));
}

#[test]
fn a_file_of_another_format_is_refused_with_a_clear_message_and_another_build_is_only_warned_of() {
    let (bytes, ..) = record(3., "");
    let mut newer = bytes.clone();
    newer[8..12].copy_from_slice(&3u32.to_le_bytes());
    let error = Replay::parse(newer.clone()).err().unwrap();
    assert_eq!(error, Error::Format { found: 3 });
    let text = error.to_string();
    assert!(text.contains("format 3") && text.contains("format 2") && text.contains("update the game"), "{text}");
    assert_eq!(scan(&mut std::io::Cursor::new(&newer)).err(), Some(Error::Format { found: 3 }));
    let mut older = bytes.clone();
    older[8..12].copy_from_slice(&0u32.to_le_bytes());
    assert!(Replay::parse(older).err().unwrap().to_string().contains("older"));
    assert_eq!(Replay::parse(b"GIF89a this is no replay".to_vec()).err(), Some(Error::NotAReplay));
    // Another build plays, with a warning; the same build has none.
    let replay = Replay::parse(bytes).unwrap();
    assert_eq!(replay.warning("kitchen-sim test"), None);
    assert!(replay.warning("hotdog-simulator 0.2.0").unwrap().contains("kitchen-sim test"));
}

#[test]
fn a_seek_then_play_is_exactly_what_playing_from_the_start_reaches_and_its_stats_too() {
    let (bytes, ..) = record(26., "mode=ffa");
    let replay = Replay::parse(bytes).unwrap();
    let end = replay.duration();
    for at in [0, 1, 4999, 5000, 7321, 12_345, 20_000, end - 40, end] {
        let (mut sought, _) = Playhead::seek(&replay, at).unwrap();
        let (mut from_start, _) = Playhead::seek(&replay, 0).unwrap();
        from_start.advance(&replay, at);
        assert_eq!(sought.ms, from_start.ms, "at {at}");
        assert!(sought.decoder.state() == from_start.decoder.state(), "the same state at {at} ms");
        assert_eq!(sought.tally, from_start.tally, "the same stats at {at} ms");
        // And they go on alike.
        let later = (at + 3456).min(end);
        let a: Vec<u32> = sought.advance(&replay, later).iter().map(|p| p.ms).collect();
        let b: Vec<u32> = from_start.advance(&replay, later).iter().map(|p| p.ms).collect();
        assert_eq!(a, b);
        assert!(sought.decoder.state() == from_start.decoder.state() && sought.tally == from_start.tally, "still alike at {later} ms");
    }
    // Free For All's bots shoot: rounds were fired and the markers name kills.
    let (_, _, head) = play_all(&replay);
    assert!(head.tally.lines.iter().map(|l| l.shots).sum::<u32>() > 0, "{:?}", head.tally.lines);
}

/// A hotdog in a scripted frame.
fn dog(id: u32, hp: f32, kills: u32, swings: u32, hit: Option<(u8, u32, f32, f32)>) -> Seat {
    let last_damage = hit.map_or(Damage::default(), |(kind, attacker, time, amount)| Damage { kind, attacker, time, amount, ..Default::default() });
    let player = Player { id, name: ["", "ALICE", "BOB", "CAROL"][id as usize].into(), hp, alive: hp > 0., kills, swings, last_damage, weapon: 1, ammo: [-1, 7, 0, 0, 0, 0, 0, 0], ..Default::default() };
    Seat { player, ..Default::default() }
}
fn shot(id: u32, owner: u32) -> Shot {
    Shot { id, owner, a: [0.; 3], b: [0.; 3], time: 0., weapon: 1, projectile: id }
}

#[test]
fn the_stats_of_a_scripted_match_count_its_shots_hits_damage_kills_deaths_and_streaks() {
    // Alice fires four rounds and hits Bob three times (30, 45, 25: dead); Carol strikes Alice twice (40, 60: dead).
    let script: Vec<(Vec<Seat>, Vec<Shot>, &str, Option<u32>)> = vec![
        (vec![dog(1, 100., 0, 0, None), dog(2, 100., 0, 0, None), dog(3, 100., 0, 0, None)], vec![], "FreeForAll", None),
        (vec![dog(1, 100., 0, 0, None), dog(2, 100., 0, 0, None), dog(3, 100., 0, 0, None)], vec![shot(1, 1), shot(2, 1)], "FreeForAll", None),
        (vec![dog(1, 100., 0, 0, None), dog(2, 70., 0, 0, Some((1, 1, 0.066, 30.))), dog(3, 100., 0, 0, None)], vec![], "FreeForAll", None),
        (vec![dog(1, 100., 0, 0, None), dog(2, 25., 0, 0, Some((1, 1, 0.1, 45.))), dog(3, 100., 0, 0, None)], vec![shot(3, 1)], "FreeForAll", None),
        (vec![dog(1, 100., 1, 0, None), dog(2, 0., 0, 0, Some((1, 1, 0.133, 25.))), dog(3, 100., 0, 0, None)], vec![shot(4, 1)], "FreeForAll", None),
        (vec![dog(1, 60., 1, 0, Some((6, 3, 0.166, 40.))), dog(2, 0., 0, 0, Some((1, 1, 0.133, 25.))), dog(3, 100., 0, 1, None)], vec![], "FreeForAll", None),
        (vec![dog(1, 0., 1, 0, Some((6, 3, 0.2, 60.))), dog(2, 0., 0, 0, Some((1, 1, 0.133, 25.))), dog(3, 100., 1, 2, None)], vec![], "FreeForAll", None),
        (vec![dog(1, 0., 1, 0, Some((6, 3, 0.2, 60.))), dog(2, 0., 0, 0, Some((1, 1, 0.133, 25.))), dog(3, 100., 1, 2, None)], vec![], "RoundOver", Some(3)),
    ];
    let header = Header { format: FORMAT, keyframe_ms: 100, ..Default::default() };
    let mut w = Writer::new(Vec::new(), &header).unwrap();
    for (i, (players, shots, stage, winner)) in script.into_iter().enumerate() {
        let frame = Frame { players, shots, stage: stage.into(), winner, round: 1, elapsed: i as f32 / 30., ..Default::default() };
        w.frame(i as u32 * 33, i as u32 * 2 + 2, &frame, &frame.state()).unwrap();
    }
    let (bytes, summary) = w.finish(0).unwrap();
    assert_eq!(summary.winner.as_deref(), Some("CAROL"));
    let replay = Replay::parse(bytes).unwrap();
    assert!(replay.chunks.len() >= 2, "short keyframes: the stats cross a chunk");
    let (_, _, head) = play_all(&replay);
    let t = &head.tally;
    let (a, b, c) = (t.line(1).unwrap(), t.line(2).unwrap(), t.line(3).unwrap());
    assert_eq!((a.shots, a.hits, a.dealt, a.kills, a.deaths, a.streak, a.best, a.taken), (4, 3, 10_000, 1, 1, 0, 1, 10_000), "{a:?}");
    assert_eq!(a.accuracy(), Some(0.75));
    assert_eq!((b.shots, b.hits, b.kills, b.deaths, b.taken, b.alive, b.hp), (0, 0, 0, 1, 10_000, false, 0.));
    assert_eq!((c.swings, c.hits, c.dealt, c.kills, c.deaths, c.streak, c.best, c.taken), (2, 2, 10_000, 1, 0, 1, 1, 0), "{c:?}");
    assert_eq!(c.accuracy(), Some(1.));
    assert_eq!((a.name.as_str(), a.weapon, a.magazine), ("ALICE", 1, 7));
    let marks: Vec<(Mark, u32, u32)> = replay.markers().map(|m| (m.kind, m.by, m.to)).collect();
    assert_eq!(marks, vec![(Mark::Kill, 1, 2), (Mark::Kill, 3, 1), (Mark::RoundEnd, 4, 1)]);
    assert_eq!(summary.lines, t.lines);
    // A seek past a keyframe has the same stats.
    let (mut sought, _) = Playhead::seek(&replay, 200).unwrap();
    sought.advance(&replay, u32::MAX);
    assert_eq!(&sought.tally, t);
}

#[test]
fn a_chunk_holds_only_a_few_seconds_so_playing_keeps_one_chunk_in_memory() {
    let (bytes, states, _) = record(31., "");
    let replay = Replay::parse(bytes).unwrap();
    for c in &replay.chunks {
        assert!(c.end - c.start < KEYFRAME_MS, "{c:?}");
    }
    let (mut head, _) = Playhead::seek(&replay, 0).unwrap();
    for to in (0..replay.duration()).step_by(1000) {
        head.advance(&replay, to);
        let frames = head.records.iter().filter(|t| matches!(t.record, Record::Frame(_))).count();
        assert!(frames <= KEYFRAME_MS as usize * 30 / 1000 + 1, "{frames} frames held");
    }
    let frames: usize = (0..replay.chunks.len()).map(|i| replay.chunk(i).unwrap().records.iter().filter(|t| matches!(t.record, Record::Frame(_))).count()).sum();
    assert_eq!(states.len(), frames);
}

/// The sizes the report gives: bytes a minute of a siege and of Free For All, five bots each.
#[test]
#[ignore]
fn sizes() {
    for rules in ["", "mode=ffa"] {
        let (bytes, ..) = record(60., rules);
        let raw: usize = {
            let replay = Replay::parse(bytes.clone()).unwrap();
            (0..replay.chunks.len()).map(|i| replay.chunk(i).unwrap().records.iter().map(|t| match &t.record { Record::Frame(b) => b.len(), Record::Text(s) => s.len() }).sum::<usize>()).sum()
        };
        println!("{} a minute: {} bytes in the file, {} bytes of frames and texts before compression", if rules.is_empty() { "siege" } else { rules }, bytes.len(), raw);
    }
}

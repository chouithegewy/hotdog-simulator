// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The players' own views: a batch reads back as it was kept (to its units), every event too; the server's checks
//! refuse what is wrong; a replay keeps tracks beside its chunks and puts them on its clock; an old file plays.
use super::*;
use crate::feast::Damage;
use crate::replay::{self, Replay};

/// A frame of a player walking, bobbing, turning past the back (yaw crossing ±π), firing now and then.
fn walking(i: usize, hz: f64) -> Sample {
    let t = i as f64 * 1000. / hz;
    let s = t / 1000.;
    let yaw = ((s * 1.7) as f32 + 3.0).rem_euclid(TAU) - std::f32::consts::PI;
    let bob = (s * 9.).sin() as f32 * 0.004;
    let firing = (i / 20) % 3 == 0;
    Sample {
        t: 123_456.25 + t,
        now: 40. + s,
        dt: (1. / hz) as f32,
        view: view::FIRST,
        flags: flag::ALIVE | flag::PLAYING | if firing { flag::SCOPED } else { 0 },
        cam: [0.5 + s as f32 * 0.6, 1.1 + bob, -2.0 + s as f32 * 0.3],
        turn: [yaw, -0.2 + 0.1 * (s as f32).sin(), if firing { 0.01 * (s as f32 * 71.).sin() } else { 0. }],
        fov: if firing { 0.6 } else { 1.396 },
        at: [0.5 + s as f32 * 0.6, 0.97, -2.0 + s as f32 * 0.3],
        yaw,
        pitch: -0.2,
        eye: 0.135,
        gun: 5,
        hold: [0.045, -0.047 + if firing { 0.003 } else { 0. }, -0.105],
        tilt: [0., if firing { 0.05 } else { 0. }, 0.],
        weapon: 5,
        aim: if firing { 1. } else { 0. },
        hit: 0.,
        reload: 0,
        progress: 0.,
        clap: 0.55,
        stim: 0.,
        hp: 87.5,
        ammo: [12, 3, 0, 1, 0, 30 - (i / 40) as i32, 0],
        reserve: [48, 9, 0, 2, 0, 90, 0],
        kills: (i / 300) as u32,
        score: 0,
        burst: 3,
        bursts: 3,
        buttons: if firing { 1 << 7 } else { 1 },
        look: [(s * 3.).sin() as f32 * 4., 0.25],
        quality: 2,
    }
}
fn every_event() -> Vec<Event> {
    vec![
        // (Angles a whole number of the units kept: they read back exactly.)
        Event::Shot { weapon: 5, yaw: -TAU / 4., pitch: TAU / 16. },
        Event::Reload { gun: 1, phase: 0 },
        Event::Reload { gun: 1, phase: 2 },
        Event::Weapon { to: 2 },
        Event::Scope { up: true },
        Event::Melee { weapon: 1 },
        Event::Move { style: 3, mv: 7 },
        Event::Hurt { damage: Damage { kind: 1, attacker: 3, p: [1., 2., 3.], d: [0., 0., -1.], time: 12.5, amount: 22.25 } },
        Event::Dealt { victim: 4, kill: true, at: [0.25, 1.0, -2.5] },
        Event::Kills,
        Event::Final { victim: 2, at: [1., 1., 1.] },
        Event::Slow { shown_only: true, focus: Some([0.5, 1., 0.5]) },
        Event::Slow { shown_only: false, focus: None },
        Event::Lens { smudges: 5 },
        Event::Pickup { index: 3 },
        Event::Carry { what: 2, pose: 6 },
        Event::Card { what: 2, seat: 4 },
        Event::Quality { level: 3, name: "Low".into() },
        Event::Seed { what: "cards".into(), value: u64::MAX - 5 },
        Event::Note { text: "hello".into() },
    ]
}
fn near(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within
}
fn angle_near(a: f32, b: f32, within: f32) -> bool {
    ((a - b + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI).abs() <= within
}

#[test]
fn a_batch_reads_back_every_sample_as_kept_and_every_event_exactly() {
    let mut batch = Batch::default();
    for i in 0..120 {
        batch.samples.push(walking(i, 120.));
        if i % 6 == 0 {
            for e in every_event() {
                batch.event(e);
            }
        }
    }
    let bytes = batch.encode();
    assert!(bytes.len() < BATCH, "{} bytes", bytes.len());
    let back = Batch::decode(&bytes).unwrap();
    assert_eq!(back.samples.len(), batch.samples.len());
    for (a, b) in batch.samples.iter().zip(&back.samples) {
        // Exactly what was kept, and within the units of what was given.
        assert_eq!(*b, a.kept());
        assert_eq!(b.kept(), *b, "keeping is idempotent");
        assert!((a.t - b.t).abs() <= 1. / 128. && (a.now - b.now).abs() <= 1e-5);
        for k in 0..3 {
            assert!(near(a.cam[k], b.cam[k], 0.5 / 65536. + 1e-6), "camera {k}: {} {}", a.cam[k], b.cam[k]);
            assert!(angle_near(a.turn[k], b.turn[k], TAU / (1 << 21) as f32 + 1e-6), "turn {k}: {} {}", a.turn[k], b.turn[k]);
            assert!(near(a.hold[k], b.hold[k], 0.5 / 262_144. + 1e-7));
        }
        assert!(near(a.fov, b.fov, 0.5 / 65536. + 1e-6));
        assert_eq!((a.view, a.flags, a.gun, a.weapon, a.ammo, a.reserve, a.kills, a.buttons, a.quality), (b.view, b.flags, b.gun, b.weapon, b.ammo, b.reserve, b.kills, b.buttons, b.quality));
    }
    assert_eq!(back.events, batch.events);
}

#[test]
fn a_still_frame_costs_a_few_bytes_and_a_moving_one_a_couple_of_dozen() {
    let one = |samples: Vec<Sample>| Batch { samples, events: vec![] }.raw().len();
    let still: Vec<Sample> = (0..60).map(|i| Sample { t: i as f64 * 16.666, now: i as f64 / 60., dt: 1. / 60., ..walking(0, 60.) }).collect();
    let moving: Vec<Sample> = (0..60).map(|i| walking(i, 60.)).collect();
    let (still, moving) = (one(still), one(moving));
    assert!(still < 60 * 8 + 120, "{still} bytes for a still second");
    assert!(moving < 60 * 40, "{moving} bytes for a moving second");
}

#[test]
fn the_server_refuses_what_is_too_big_broken_out_of_order_too_many_or_outside_the_match() {
    let good: Vec<Sample> = (0..60).map(|i| walking(i, 60.)).collect();
    let span = (good[0].t, good[59].t);
    let encode = |samples: Vec<Sample>| Batch { samples, events: vec![] }.encode();
    let kept = (good[0].kept().t, good[59].kept().t);
    assert_eq!(check(&encode(good.clone()), span.0 - 1., span.1 + 1.).unwrap(), kept);
    // Outside the match (the clock far ahead of the server's).
    assert!(check(&encode(good.clone()), span.0 + 10., f64::MAX).is_err());
    assert!(check(&encode(good.clone()), 0., span.1 - 10.).is_err());
    // Backwards.
    let mut back = good.clone();
    back.swap(10, 11);
    assert!(check(&encode(back), 0., f64::MAX).unwrap_err().0.contains("backwards"));
    // More frames than the rate allows over its span.
    let crowded: Vec<Sample> = (0..400).map(|i| Sample { t: 1000. + i as f64 * 0.5, ..walking(i, 60.) }).collect();
    assert!(check(&encode(crowded), 0., f64::MAX).unwrap_err().0.contains("rate"));
    // Not a batch, empty, too large, inflating too far.
    assert!(check(b"nonsense", 0., f64::MAX).is_err());
    assert!(check(&encode(vec![]), 0., f64::MAX).is_err());
    assert!(check(&vec![0u8; BATCH + 1], 0., f64::MAX).is_err());
    let bomb = miniz_oxide::deflate::compress_to_vec_zlib(&vec![0u8; INFLATE * 2], 10);
    assert!(bomb.len() < BATCH && check(&bomb, 0., f64::MAX).is_err());
    // An event on a frame the batch does not have.
    let mut b = Batch { samples: good[..2].to_vec(), events: vec![] };
    b.events.push(Timed { at: 5, event: Event::Kills });
    assert!(check(&b.encode(), 0., f64::MAX).is_err());
    // A header: too large, or of another format.
    assert!(Header::parse(&"x".repeat(HEADER + 1)).is_err());
    assert!(Header::parse(r#"{"format": 9}"#).is_err());
    assert_eq!(Header::parse(&Header { player: 3, ..Default::default() }.text()).unwrap().player, 3);
}

#[test]
fn base64_goes_there_and_back() {
    for n in 0..70 {
        let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
        assert_eq!(unbase64(&base64(&bytes)).unwrap(), bytes);
    }
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert!(unbase64("ab$d").is_err());
}

#[test]
fn a_view_is_exact_at_its_frames_and_between_them_moves_the_short_way_round() {
    let header = Header { player: 2, rate: 60, ..Default::default() };
    let batches: Vec<Vec<u8>> = (0..3).map(|b| Batch { samples: (b * 60..b * 60 + 60).map(|i| walking(i, 60.)).collect(), events: vec![Timed { at: 0, event: Event::Kills }] }.encode()).collect();
    let track = Track::from_batches(header, 123_456.25, batches.iter().rev().map(|b| b.as_slice()));
    assert_eq!(track.samples.len(), 180);
    assert_eq!(track.events.len(), 3);
    assert!(track.events.windows(2).all(|w| w[0].at < w[1].at), "the batches in order");
    assert!((track.samples[0].t).abs() < 1e-9, "on the replay's clock");
    for s in &track.samples {
        assert_eq!(track.view(s.t), Some(*s), "exact at its own time");
    }
    // Half way between two frames: half way along, turned the short way (one pair crosses the back of the turn).
    for i in 0..179 {
        let (a, b) = (&track.samples[i], &track.samples[i + 1]);
        let mid = track.view((a.t + b.t) / 2.).unwrap();
        assert!(near(mid.cam[0], (a.cam[0] + b.cam[0]) / 2., 1e-5));
        let step = (b.turn[0] - a.turn[0]).abs().min(TAU - (b.turn[0] - a.turn[0]).abs());
        assert!(angle_near(mid.turn[0], a.turn[0], step / 2. + 1e-4) && angle_near(mid.turn[0], b.turn[0], step / 2. + 1e-4), "{} {} {}", a.turn[0], mid.turn[0], b.turn[0]);
    }
    assert_eq!(track.view(-1.), None);
    assert_eq!(track.view(track.samples[179].t + 1.), None);
    let events: Vec<_> = track.events_between(-1., 999.).collect();
    assert_eq!(events.len(), 1);
}

/// A replay of a scripted game (`frames` of nobody doing anything) with two players' tracks written beside it.
fn replay_with_tracks(drop_second: bool) -> (Vec<u8>, Vec<Sample>) {
    let map = crate::map::Map::bundled();
    let header = replay::Header::new("kitchen-sim test", "server", &map, "siege", "", vec![], 1_700_000_000);
    let mut w = replay::Writer::new(Vec::new(), &header).unwrap();
    let feast = crate::feast::Feast::with_map(map, true);
    let frame = replay::snapshot(&feast, None, |_| String::new());
    let mut kept = vec![];
    let offset = 123_456.25;
    w.pov_header(2, offset, &Header { player: 2, ..Default::default() }.text()).unwrap();
    w.pov_header(3, offset, &Header { player: 3, ..Default::default() }.text()).unwrap();
    for second in 0..12u32 {
        for k in 0..30 {
            let tick = second * 60 + k * 2 + 2;
            let mut f = frame.clone();
            f.elapsed = tick as f32 / 60.;
            w.frame(tick * 1000 / 60, tick, &f, &f.state()).unwrap();
        }
        let samples: Vec<Sample> = (second as usize * 60..second as usize * 60 + 60).map(|i| walking(i, 60.)).collect();
        kept.extend(samples.iter().map(|s| s.kept()));
        let batch = Batch { samples, events: vec![Timed { at: 3, event: Event::Shot { weapon: 5, yaw: 0.5, pitch: 0. } }] }.encode();
        w.pov(2, &batch).unwrap();
        if second == 6 && drop_second {
            w.pov_drop(3, "too large").unwrap();
        }
        w.pov(3, &batch).unwrap();
        // A block that does not read: passed over.
        w.pov(2, b"garbage").unwrap();
    }
    (w.finish(1_700_000_100).unwrap().0, kept)
}

#[test]
fn a_replay_keeps_each_players_track_beside_its_chunks_on_its_own_clock() {
    let (bytes, kept) = replay_with_tracks(true);
    let replay = Replay::parse(bytes.clone()).unwrap();
    assert!(!replay.truncated && replay.chunks.len() >= 2);
    assert_eq!(replay.povs.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
    let two = replay.track(2).unwrap();
    assert_eq!(two.samples.len(), kept.len(), "the garbage blocks are left out");
    for (a, b) in two.samples.iter().zip(&kept) {
        assert_eq!(Sample { t: a.t + 123_456.25, ..*a }, *b);
    }
    assert_eq!(two.events.len(), 12);
    assert_eq!(two.header.player, 2);
    // The second was dropped half way: kept up to there, and why.
    let three = replay.track(3).unwrap();
    assert_eq!(three.samples.len(), 6 * 60);
    assert_eq!(three.dropped.as_deref(), Some("too large"));
    // The listing names them without inflating anything; the chunks play as before.
    let listing = replay::scan(&mut std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(listing.povs, vec![2, 3]);
    let (mut head, _) = replay::Playhead::seek(&replay, 0).unwrap();
    head.advance(&replay, u32::MAX);
    assert!(head.done(&replay) && head.stopped.is_none());
    assert_eq!(replay.header.format, replay::FORMAT);
}

#[test]
fn a_format_1_file_from_before_tracks_still_plays_and_has_none() {
    // Format 1 was format 2 without its players' views: the same blocks, numbered 1.
    let (bytes, _) = replay_with_tracks(false);
    let plain = Replay::parse(bytes).unwrap();
    let map = crate::map::Map::bundled();
    let header = replay::Header { format: 1, ..replay::Header::new("kitchen-sim test", "server", &map, "siege", "", vec![], 1) };
    let mut w = replay::Writer::new(Vec::new(), &header).unwrap();
    let feast = crate::feast::Feast::with_map(map, true);
    let frame = replay::snapshot(&feast, None, |_| String::new());
    for tick in (2..600).step_by(2) {
        w.frame(tick * 1000 / 60, tick, &frame, &frame.state()).unwrap();
    }
    let mut old = w.finish(2).unwrap().0;
    old[8..12].copy_from_slice(&1u32.to_le_bytes());
    let replay = Replay::parse(old.clone()).unwrap();
    assert!(replay.povs.is_empty() && replay.track(1).is_none());
    assert_eq!(replay.header.format, 1);
    let (mut head, _) = replay::Playhead::seek(&replay, 0).unwrap();
    head.advance(&replay, u32::MAX);
    assert!(head.done(&replay) && head.stopped.is_none());
    assert!(replay::scan(&mut std::io::Cursor::new(&old)).unwrap().povs.is_empty());
    assert!(!plain.povs.is_empty());
}

/// Bytes a minute of a track: 60 and 120 frames a second of walking, turning and firing (the report's figures come
/// from a real game: the game's replay tests).
#[test]
#[ignore]
fn sizes() {
    for hz in [60., 120., 144.] {
        let rate = (hz as u32).min(RATE) as f64;
        let mut total = 0;
        for second in 0..60 {
            let samples: Vec<Sample> = (0..rate as usize).map(|i| walking(second * rate as usize + i, rate)).collect();
            total += Batch { samples, events: vec![] }.encode().len() + 64;
        }
        println!("{hz} frames a second ({rate} kept): {total} bytes a minute");
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The viewer and the single-player recorder, in the game as the tests build it (arena.rs `test_app`): a game records
//! itself; a replay plays back where everyone was, their health and their scores; a seek is what playing there from the
//! start reaches; the speeds move the clock as they say; the views put the camera where they say.
use super::*;
use crate::arena::{test_app, test_frame};
use kitchen_sim::feast::Feast;
use kitchen_sim::map::Map;
use kitchen_sim::replay::{self, Header, Writer, say};

/// A game of bots (Free For All: they fight) on the bundled map recorded as the server would, for `seconds`: the file,
/// and each frame's time and what it said of the players (quantised as sent).
pub(crate) fn recorded(seconds: f32) -> (Vec<u8>, Vec<(u32, Frame)>) {
    let map = Map::bundled();
    let mut feast = Feast::with_map(map.clone(), true);
    feast.set_rules(kitchen_sim::rules::Rules::parse("mode=ffa"));
    let first = replay::snapshot(&feast, None, |_| String::new());
    let header = Header::new(store::BUILD, "server", &map, "ffa", &feast.rules.text(), replay::roster(&first), 1_800_000_000);
    let mut w = Writer::new(Vec::new(), &header).unwrap();
    let mut frames = vec![];
    let mut said = String::new();
    for tick in 1..=(seconds * 60.) as u32 {
        feast.tick(1. / 60.);
        if tick % 2 != 0 {
            continue;
        }
        let ms = (tick as u64 * 1000 / 60) as u32;
        let ammo = say::ammo(&feast.stocks());
        if ammo != said {
            w.text(ms, &ammo);
            said = ammo;
        }
        let frame = replay::snapshot(&feast, None, |_| String::new());
        let state = frame.state();
        w.frame(ms, tick, &frame, &state).unwrap();
        frames.push((ms, Frame::from_state(&state, frame.elapsed)));
    }
    (w.finish(1_800_000_100).unwrap().0, frames)
}
/// A file of the build's own beside the test binary.
fn file(name: &str, bytes: &[u8]) -> String {
    let path = std::env::current_exe().unwrap().with_file_name(format!("replay-test-{name}-{}.replay", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into()
}
/// The viewer on `path`, as main.rs runs it: the replay fed to the client before it reads, the arena, the camera after.
fn viewer(path: &str) -> App {
    let mut app = test_app("");
    {
        let mut o = app.world_mut().resource_mut::<crate::runtime::Options>();
        (o.replay, o.demo, o.connect) = (path.into(), true, crate::runtime::REPLAY.into());
    }
    app.world_mut().resource_mut::<Game>().demo = true;
    app.world_mut().insert_resource(crate::lobby::Lobby::default());
    app.init_resource::<Network>()
        .init_resource::<Bindings>()
        .init_resource::<crate::settings::Settings>()
        .init_resource::<crate::slowmo::SlowMo>()
        .init_resource::<Time<Virtual>>()
        .init_resource::<list::List>()
        .insert_resource(Viewer::new(path))
        .add_systems(Update, (feed, crate::network::receive, crate::arena::tick, crate::network::send, camera).chain());
    app
}
fn frames(app: &mut App, n: usize, dt: f32) {
    for _ in 0..n {
        test_frame(app, dt);
    }
}
/// Where the timeline draws hotdog `id` now.
fn drawn(app: &App, id: u32) -> Option<Vec3> {
    let g = app.world().resource::<Game>();
    g.online.timeline.player(id, g.online.hz).map(|s| s.p)
}
/// Where hotdog `id` was at `ms` in the recording, between the frames either side.
fn truth(frames: &[(u32, Frame)], id: u32, ms: f64) -> Vec3 {
    let i = frames.iter().rposition(|(m, _)| *m as f64 <= ms).unwrap_or(0);
    let at = |k: usize| Vec3::from_array(frames[k].1.players.iter().find(|s| s.player.id == id).unwrap().player.p);
    match frames.get(i + 1) {
        Some((mb, _)) => {
            let ma = frames[i].0 as f64;
            at(i).lerp(at(i + 1), ((ms - ma) / (*mb as f64 - ma)).clamp(0., 1.) as f32)
        }
        None => at(i),
    }
}

#[test]
fn a_single_player_game_records_itself_into_the_replays_folder() {
    let _one = store::FOLDER.lock().unwrap_or_else(|e| e.into_inner());
    let dir = store::local_dir().unwrap();
    // (Beside the test binary: never the player's own folder.)
    assert!(dir.starts_with(std::env::current_exe().unwrap().parent().unwrap()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = test_app("");
    // (Options as a test makes them record nothing, as `--no-record` does: this one asks to.)
    assert!(!app.world().resource::<crate::runtime::Options>().record);
    app.world_mut().resource_mut::<crate::runtime::Options>().record = true;
    app.init_resource::<crate::prefs::Prefs>()
        .init_resource::<Bindings>()
        .init_resource::<crate::settings::Settings>()
        .init_resource::<record::Recorder>()
        .add_systems(Update, (crate::arena::tick, record::sample).chain());
    frames(&mut app, 200, 1. / 30.);
    assert!(app.world().resource::<record::Recorder>().recording(), "a round being played is being recorded");
    let last = app.world().resource::<Game>().feast.players.clone();
    app.world_mut().resource_mut::<record::Recorder>().finish();
    let names = app.world().resource::<record::Recorder>().finished.clone();
    assert_eq!(names.len(), 1);
    let listed = store::list_local();
    assert_eq!(listed.len(), 1, "{listed:?}");
    let replay = Replay::open(std::path::Path::new(&listed[0].key)).unwrap();
    assert_eq!((replay.header.source.as_str(), replay.header.build.as_str(), replay.truncated), ("offline", store::BUILD, false));
    assert_eq!(replay.header.roster.iter().filter(|m| !m.bot).count(), 1, "single player's own hotdog is a person's");
    let seconds = replay.duration() as f32 / 1000.;
    assert!((5.5..7.).contains(&seconds), "{seconds} s recorded of about 6.6");
    // Its last frame is the round as it stood at the last sample.
    let (mut head, mut played) = Playhead::seek(&replay, 0).unwrap();
    played.extend(head.advance(&replay, u32::MAX));
    let frames: Vec<&Frame> = played.iter().filter_map(|p| p.frame.as_ref()).collect();
    assert!(frames.len() > 150, "{} frames: thirty a second", frames.len());
    for (seen, was) in frames.last().unwrap().players.iter().zip(&last) {
        assert!((Vec3::from_array(seen.player.p) - Vec3::from_array(was.p)).length() < 0.05, "{} where it was", was.name);
        assert_eq!(seen.player.score, was.score);
    }
    // Switched off, nothing more is recorded.
    app.world_mut().resource_mut::<record::Recorder>().set(None, false);
    frames_more(&mut app);
    assert!(!app.world().resource::<record::Recorder>().recording());
    let _ = std::fs::remove_dir_all(&dir);
}
fn frames_more(app: &mut App) {
    frames(app, 10, 1. / 30.);
}

#[test]
fn a_replay_plays_back_where_everyone_was_their_health_and_their_scores() {
    let (bytes, truth_frames) = recorded(14.);
    let path = file("positions", &bytes);
    let mut app = viewer(&path);
    frames(&mut app, 3, 1. / 60.);
    {
        let g = app.world().resource::<Game>();
        assert!(g.network && !g.demo, "watched as a client of the file, from the first player's eyes with its display");
        assert_eq!(g.feast.level.map.name, Map::bundled().name);
    }
    let mut checked = 0;
    for step in 0..600 {
        test_frame(&mut app, 1. / 60.);
        if step % 50 != 49 {
            continue;
        }
        let v = app.world().resource::<Viewer>();
        let clock = v.clock;
        // Where each hotdog is drawn is where it was then.
        for id in 1..=5 {
            if let Some(at) = drawn(&app, id) {
                let was = truth(&truth_frames, id, clock);
                assert!((at - was).length() < 0.02, "hotdog {id} at {clock:.0} ms: drawn {at}, was {was}");
                checked += 1;
            }
        }
        // The frame at the clock says what was said then: health, score, kills.
        let frame = v.frame.as_ref().unwrap();
        let ms = truth_frames.iter().rev().find(|(m, _)| *m as f64 <= clock).unwrap();
        for (seen, was) in frame.players.iter().zip(&ms.1.players) {
            assert_eq!((seen.player.hp, seen.player.score, seen.player.kills, seen.player.alive), (was.player.hp, was.player.score, was.player.kills, was.player.alive));
        }
    }
    assert!(checked > 30, "{checked} places compared");
    // The client decoded every frame it was handed, none broken.
    let net = app.world().resource::<Network>();
    assert!(net.frames > 250 && net.broken == 0, "{} frames, {} broken", net.frames, net.broken);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_seek_then_play_is_what_playing_from_the_start_reaches() {
    let (bytes, _) = recorded(16.);
    let path = file("seek", &bytes);
    // From the start to 9.5 s.
    let mut a = viewer(&path);
    frames(&mut a, 2, 1. / 60.);
    for _ in 0..(9.5 * 60.) as usize - 2 {
        test_frame(&mut a, 1. / 60.);
    }
    // Sought: to 3 s, then to 9 s, and on to the same moment.
    let mut b = viewer(&path);
    frames(&mut b, 2, 1. / 60.);
    let target = a.world().resource::<Viewer>().clock;
    b.world_mut().resource_mut::<Viewer>().seek = Some(3000.);
    frames(&mut b, 10, 1. / 60.);
    b.world_mut().resource_mut::<Viewer>().seek = Some(target - 500.);
    frames(&mut b, 10, 1. / 60.);
    let behind = target - b.world().resource::<Viewer>().clock;
    frames(&mut b, (behind / 1000. * 60.).round() as usize, 1. / 60.);
    let (va, vb) = (a.world().resource::<Viewer>(), b.world().resource::<Viewer>());
    assert!((va.clock - vb.clock).abs() < 1., "{} and {}", va.clock, vb.clock);
    assert!(vb.seeks >= 3, "the start, and two seeks");
    assert_eq!(va.now.as_ref().unwrap().tally, vb.now.as_ref().unwrap().tally, "the same stats");
    let (fa, fb) = (va.frame.as_ref().unwrap(), vb.frame.as_ref().unwrap());
    assert_eq!(fa.tick, fb.tick);
    for (x, y) in fa.players.iter().zip(&fb.players) {
        assert_eq!((x.player.p, x.player.hp, x.player.score), (y.player.p, y.player.hp, y.player.score));
    }
    for id in 1..=5 {
        let (pa, pb) = (drawn(&a, id), drawn(&b, id));
        assert!(pa.zip(pb).is_none_or(|(pa, pb)| (pa - pb).length() < 0.002), "hotdog {id}: {pa:?} and {pb:?}");
    }
    let (ga, gb) = (a.world().resource::<Game>(), b.world().resource::<Game>());
    assert_eq!(ga.roster.iter().map(|p| (p.id, p.hp, p.score)).collect::<Vec<_>>(), gb.roster.iter().map(|p| (p.id, p.hp, p.score)).collect::<Vec<_>>());
    assert_eq!(b.world().resource::<Network>().broken, 0, "a seek never desyncs the client");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_speeds_move_the_clock_a_quarter_a_half_once_and_twice_as_fast_and_pause_stops_it() {
    let (bytes, _) = recorded(12.);
    let path = file("speeds", &bytes);
    let mut app = viewer(&path);
    frames(&mut app, 2, 1. / 60.);
    for (speed, want) in [(0, 250.), (1, 500.), (2, 1000.), (3, 2000.)] {
        app.world_mut().resource_mut::<Viewer>().speed = speed;
        let before = app.world().resource::<Viewer>().clock;
        frames(&mut app, 60, 1. / 60.);
        let moved = app.world().resource::<Viewer>().clock - before;
        assert!((moved - want).abs() < 1., "{} moved {moved} ms in a second", SPEEDS[speed]);
        assert_eq!(app.world().resource::<crate::slowmo::SlowMo>().replay, Some(SPEEDS[speed]), "the round goes at the speed too");
    }
    // The keys: slower and faster stop at the ends.
    let press = |app: &mut App, key: KeyCode| {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        test_frame(app, 1. / 60.);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
        test_frame(app, 1. / 60.);
    };
    for _ in 0..5 {
        press(&mut app, KeyCode::BracketLeft);
    }
    assert_eq!(app.world().resource::<Viewer>().speed, 0);
    for _ in 0..5 {
        press(&mut app, KeyCode::BracketRight);
    }
    assert_eq!(app.world().resource::<Viewer>().speed, 3);
    // Paused, the clock stands; a frame step moves it one frame.
    press(&mut app, KeyCode::KeyP);
    let paused = app.world().resource::<Viewer>().clock;
    assert!(app.world().resource::<Viewer>().paused);
    frames(&mut app, 30, 1. / 60.);
    assert_eq!(app.world().resource::<Viewer>().clock, paused);
    press(&mut app, KeyCode::Period);
    let stepped = app.world().resource::<Viewer>().clock - paused;
    assert!((stepped - 1000. / 30.).abs() < 0.01, "{stepped}");
    // At the end it stops, and play starts again from the beginning.
    app.world_mut().resource_mut::<Viewer>().paused = false;
    frames(&mut app, 400, 1. / 30.);
    let v = app.world().resource::<Viewer>();
    assert!(v.ended && v.paused && v.clock == v.duration());
    press(&mut app, KeyCode::KeyP);
    assert!(app.world().resource::<Viewer>().clock < 200.);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_views_are_first_person_following_and_flying_and_the_keys_change_whose() {
    let (bytes, _) = recorded(10.);
    let path = file("views", &bytes);
    let mut app = viewer(&path);
    frames(&mut app, 60, 1. / 60.);
    let cam = |app: &mut App| {
        let world = app.world_mut();
        *world.query_filtered::<&Transform, With<crate::maya::HotdogCamera>>().iter(world).next().unwrap()
    };
    let press = |app: &mut App, key: KeyCode| {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        test_frame(app, 1. / 60.);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
    };
    // First person: the camera at the watched player's eyes, its own display up (not watching from nowhere).
    let watched = app.world().resource::<Viewer>().watched;
    assert_eq!(app.world().resource::<Viewer>().mode, Mode::First);
    let at = drawn(&app, watched).unwrap();
    let eye = cam(&mut app).translation;
    assert!((eye - at).length() < 0.2 && eye.y > at.y, "first person at the eyes: {eye} over {at}");
    assert_eq!(app.world().resource::<Game>().network_id, watched);
    assert!(!app.world().resource::<Game>().demo, "its display is up");
    // The next player.
    press(&mut app, KeyCode::KeyE);
    let next = app.world().resource::<Viewer>().watched;
    assert_ne!(next, watched);
    test_frame(&mut app, 1. / 60.);
    assert_eq!(app.world().resource::<Game>().network_id, next);
    let at = drawn(&app, next).unwrap();
    assert!((cam(&mut app).translation - at).length() < 0.2, "now its eyes");
    // A player by its place.
    press(&mut app, KeyCode::Digit1);
    test_frame(&mut app, 1. / 60.);
    assert_eq!(app.world().resource::<Viewer>().watched, app.world().resource::<Viewer>().players()[0]);
    // Following: behind and over it, looking at it.
    press(&mut app, KeyCode::KeyV);
    test_frame(&mut app, 1. / 60.);
    let v = app.world().resource::<Viewer>();
    assert_eq!(v.mode, Mode::Follow);
    let (id, distance) = (v.watched, v.orbit.distance);
    let at = drawn(&app, id).unwrap();
    let tr = cam(&mut app);
    assert!((tr.translation.distance(at) - distance).abs() < 0.12, "{} from it", tr.translation.distance(at));
    assert!(tr.forward().dot((at - tr.translation).normalize()) > 0.95, "looking at it");
    assert!(app.world().resource::<Game>().demo, "no first-person display while following");
    // Flying: from where the camera was, and the walk's keys fly it.
    press(&mut app, KeyCode::KeyV);
    assert_eq!(app.world().resource::<Viewer>().mode, Mode::Fly);
    let start = cam(&mut app).translation;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
    frames(&mut app, 30, 1. / 60.);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Space);
    let up = cam(&mut app).translation - start;
    assert!(up.y > 0.2 && up.y < 0.4, "half a second up at 0.6 m/s: {up}");
    // Off the counter and high over the kitchen, if it likes.
    app.world_mut().resource_mut::<Viewer>().fly.at = Vec3::new(0., 6., 0.);
    test_frame(&mut app, 1. / 60.);
    assert!(cam(&mut app).translation.y > 5.9);
    // And round to first person again.
    press(&mut app, KeyCode::KeyV);
    assert_eq!(app.world().resource::<Viewer>().mode, Mode::First);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_file_that_is_not_a_replay_or_of_another_format_says_so_and_plays_nothing() {
    let path = file("broken", b"this is not a replay at all");
    let mut app = viewer(&path);
    frames(&mut app, 3, 1. / 60.);
    let v = app.world().resource::<Viewer>();
    assert!(v.replay.is_none() && v.failed.as_deref().is_some_and(|f| f.contains("not a Hotdog Simulator replay")), "{:?}", v.failed);
    let (mut bytes, _) = recorded(2.);
    bytes[8..12].copy_from_slice(&9u32.to_le_bytes());
    let path2 = file("format", &bytes);
    let mut app = viewer(&path2);
    frames(&mut app, 3, 1. / 60.);
    assert!(app.world().resource::<Viewer>().failed.as_deref().is_some_and(|f| f.contains("format 9")));
    let _ = (std::fs::remove_file(path), std::fs::remove_file(path2));
}

/// The texts on the viewer's screen.
fn texts(world: &mut World) -> Vec<String> {
    world.query::<&Text>().iter(world).map(|t| t.0.clone()).collect()
}
/// In a browser the file comes some frames after the screen is first built: the screen then says what is played (not
/// LOADING THE REPLAY... for ever), with the timeline's markers; and one that cannot be played says so. What the page
/// is told (page.rs) follows: loading, playing, failed with its kind.
#[test]
fn the_screen_says_what_is_played_once_the_file_has_come_or_that_it_cannot_be() {
    let (bytes, _) = recorded(6.);
    let mut world = World::new();
    world.init_resource::<ButtonInput<MouseButton>>();
    world.insert_resource(Viewer::new("in a browser"));
    let mut schedule = Schedule::default();
    schedule.add_systems(ui::draw);
    schedule.run(&mut world);
    assert!(texts(&mut world).iter().any(|t| t == "LOADING THE REPLAY..."));
    assert_eq!(page::state(world.resource::<Viewer>())["state"], "loading");
    // The file comes.
    let replay = Replay::parse(bytes.clone()).unwrap();
    let map = replay.header.map_name.to_ascii_uppercase();
    world.resource_mut::<Viewer>().replay = Some(replay);
    schedule.run(&mut world);
    let shown = texts(&mut world);
    assert!(!shown.iter().any(|t| t.contains("LOADING")), "{shown:?}");
    assert!(shown.iter().any(|t| t.contains(&map) && t.contains("FREE FOR ALL")), "{shown:?}");
    let told = page::state(world.resource::<Viewer>());
    assert_eq!((told["state"].as_str(), told["duration"].as_f64().is_some_and(|d| d > 5000.)), (Some("playing"), true), "{told}");
    // One that cannot be played.
    let mut world = World::new();
    world.init_resource::<ButtonInput<MouseButton>>();
    world.insert_resource(Viewer::new("broken"));
    let mut schedule = Schedule::default();
    schedule.add_systems(ui::draw);
    schedule.run(&mut world);
    {
        let mut v = world.resource_mut::<Viewer>();
        v.failed = Some("replays/x: HTTP 404".into());
        v.failure = Some(page::failure("replays/x: HTTP 404"));
    }
    schedule.run(&mut world);
    let shown = texts(&mut world);
    assert!(shown.iter().any(|t| t == "THE REPLAY CANNOT BE PLAYED") && !shown.iter().any(|t| t.contains("LOADING")), "{shown:?}");
    assert_eq!(page::state(world.resource::<Viewer>())["kind"], "missing");
}

/// The page's controls (touch.js) do what the keys do: play and pause, the speeds, a seek to a share of the timeline,
/// five seconds on, the next player and the next view.
#[test]
fn the_pages_controls_play_pause_seek_and_change_the_view() {
    let (bytes, _) = recorded(12.);
    let path = file("page", &bytes);
    let mut app = viewer(&path);
    frames(&mut app, 30, 1. / 60.);
    let ask = |app: &mut App, word: &str| {
        let world = app.world_mut();
        let mut v = world.remove_resource::<Viewer>().unwrap();
        page::apply(world, &mut v, page::parse(word).unwrap());
        world.insert_resource(v);
    };
    ask(&mut app, "play");
    assert!(app.world().resource::<Viewer>().paused);
    ask(&mut app, "speed:3");
    ask(&mut app, "play");
    let v = app.world().resource::<Viewer>();
    assert!(!v.paused && v.speed == 3);
    ask(&mut app, "seek:0.5");
    let duration = app.world().resource::<Viewer>().duration();
    assert!((app.world().resource::<Viewer>().seek.unwrap() - duration / 2.).abs() < 1.);
    frames(&mut app, 20, 1. / 60.);
    let clock = app.world().resource::<Viewer>().clock;
    assert!((clock - duration / 2.).abs() < 1500., "sought to the middle: {clock} of {duration}");
    ask(&mut app, "ahead");
    assert!(app.world().resource::<Viewer>().seek.unwrap() <= duration);
    let (before, mode) = (app.world().resource::<Viewer>().watched, app.world().resource::<Viewer>().mode);
    ask(&mut app, "next");
    ask(&mut app, "view");
    let v = app.world().resource::<Viewer>();
    assert!(v.watched != before && v.mode == mode.next(), "{} {:?}", v.watched, v.mode);
    ask(&mut app, "stats");
    assert!(app.world().resource::<Viewer>().show.stats);
    let _ = std::fs::remove_file(path);
}

/// The bar and the stats fit the window: on a desktop as they were; on a phone held upright the bar no wider than the
/// screen, and the stats in fewer columns at a size that can be read; sideways the stats whole.
#[test]
fn the_bar_and_the_stats_fit_a_phone_held_either_way() {
    assert_eq!(ui::bar_width(1., 1280.), 640.);
    assert!(ui::bar_width(0.6, 428.) * 0.6 <= 428. - 20.);
    assert_eq!(ui::stats_fit(1., 1280.), (11, 1.));
    let (columns, scale) = ui::stats_fit(0.6, 428.);
    assert!(columns < 11 && scale >= 0.9, "{columns} {scale}");
    assert_eq!(ui::stats_fit(0.6, 926.), (11, 0.6), "sideways: every column");
    assert_eq!(ui::card_left(5, 281., 428.), Val::Px(-206.), "upright: 8 px in from the left edge");
    assert_eq!(ui::card_left(11, 798., 1280.), Val::Px(-399.), "centred");
    let (columns, scale) = ui::stats_fit(0.6, 320.);
    assert!(columns == 5 && scale >= 0.3 && scale < 0.9, "a very small screen: the fewest, smaller: {columns} {scale}");
}

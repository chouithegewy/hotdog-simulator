// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! A player's own view, kept and shown again: a scripted single-player session with a fixed frame step (walking,
//! turning, firing every gun, reloading, the scope and the rifle's optic, being hit, a kill, a death and the respawn)
//! is recorded; its view, played back, puts the camera and the field of view exactly where the track has them, frame
//! by frame, which is where the game drew them to within the track's units; the gun in hand, the hit marker, the
//! indicators and the scope are as they were; a seek finds the same; the speeds and another frame rate fall between
//! the frames. Online the view goes to the server as its messages, which kept as the server keeps them play back too.
use super::*;
use crate::arena::{test_app, test_frame};
use crate::replay::{Viewer, camera, feed, record};
use crate::damage::Hurt;
use crate::slowmo::SlowMo;
use kitchen_sim::feast::Damage;
use kitchen_sim::replay::Replay;

const DT: f32 = 1. / 60.;
/// The track's units: positions to 2^-16 m (the gun's root 2^-18), turns to 2^-20 of one, the field of view 2^-16.
const POSITION: f32 = 0.5 / 65536. + 2e-6;
const TURN: f32 = std::f32::consts::TAU / (1 << 20) as f32 * 1.5 + 2e-6;

/// What the game drew on a frame, read once it was drawn: the truth a recorded view is measured against.
#[derive(Clone, Debug)]
struct Drawn {
    elapsed: f32,
    cam: (Vec3, Quat),
    fov: f32,
    gun: Option<(Weapon, Vec3, Quat)>,
    magazine: Option<Transform>,
    marker: Option<(f32, bool)>,
    arcs: usize,
    scoped: bool,
    aim: f32,
    crosshair: String,
    slow: f32,
    weapon: Weapon,
}
#[derive(Resource, Default)]
struct Truth(Vec<Drawn>);
/// The frame's length the live session runs at.
#[derive(Resource)]
struct Step(f32);

fn crosshair(world: &mut World) -> String {
    world.query::<(&crate::arena::Hud, &Text)>().iter(world).find(|(h, _)| h.0 == 3).map_or(String::new(), |(_, t)| t.0.clone())
}
fn magazine(world: &World, root: Entity) -> Option<Transform> {
    let mut below = world.get::<Children>(root).map(|c| c.to_vec()).unwrap_or_default();
    while let Some(child) = below.pop() {
        if world.get::<crate::weapons::Magazine>(child).is_some() {
            return world.get::<Transform>(child).copied();
        }
        below.extend(world.get::<Children>(child).map(|c| c.to_vec()).unwrap_or_default());
    }
    None
}
/// What is drawn now (see `Drawn`).
fn drawn(world: &mut World) -> Option<Drawn> {
    let text = crosshair(world);
    let g = world.get_resource::<Game>()?;
    let cam = g.camera()?;
    let (_, turn, at) = world.get::<GlobalTransform>(cam)?.to_scale_rotation_translation();
    let Projection::Perspective(p) = world.get::<Projection>(cam)? else { return None };
    let gun = g.viewmodels().iter().find(|(_, e)| world.get::<Visibility>(*e) == Some(&Visibility::Visible)).map(|(w, e)| (*w, *e));
    let hurt = world.resource::<Hurt>();
    let now = world.resource::<Time<Real>>().elapsed_secs();
    Some(Drawn {
        elapsed: g.feast.elapsed,
        cam: (at, turn),
        fov: p.fov,
        gun: gun.map(|(w, e)| {
            let t = world.get::<Transform>(e).unwrap();
            (w, t.translation, t.rotation)
        }),
        magazine: gun.and_then(|(_, e)| magazine(world, e)),
        marker: hurt.marker,
        arcs: hurt.arcs.len(),
        scoped: g.scoped,
        aim: g.aim,
        crosshair: text,
        slow: world.resource::<SlowMo>().level(now),
        weapon: g.weapon,
    })
}
fn truth(world: &mut World) {
    if world.resource::<record::Recorder>().recording()
        && let Some(d) = drawn(world)
    {
        world.resource_mut::<Truth>().0.push(d);
    }
}

/// Single player by `rules` as `main` runs it, keeping its replay and its view: the arena, the reload's pose, the
/// indicators, the slow motion, the recorder; once the frame is drawn the view's sampler, then the truth.
fn live(rules: &str) -> App {
    let mut app = test_app(rules);
    app.add_plugins(bevy::transform::TransformPlugin);
    {
        let mut o = app.world_mut().resource_mut::<crate::runtime::Options>();
        (o.record, o.pov) = (true, true);
    }
    app.init_resource::<crate::prefs::Prefs>()
        .init_resource::<crate::controls::Bindings>()
        .init_resource::<crate::settings::Settings>()
        .init_resource::<record::Recorder>()
        .init_resource::<Pov>()
        .init_resource::<Shown>()
        .init_resource::<Hurt>()
        .init_resource::<SlowMo>()
        .init_resource::<Truth>()
        .add_systems(Update, (crate::arena::tick, crate::reload::follow, crate::damage::update, crate::slowmo::present, record::sample).chain())
        .add_systems(Last, (sample, truth).chain());
    app
}
fn hold(app: &mut App, key: KeyCode, on: bool) {
    let mut k = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if on { k.press(key) } else { k.release(key) }
}
fn button(app: &mut App, b: MouseButton, on: bool) {
    let mut m = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    if on { m.press(b) } else { m.release(b) }
}
fn run(app: &mut App, seconds: f32, look: Vec2) {
    let dt = app.world().get_resource::<Step>().map_or(DT, |s| s.0);
    for _ in 0..(seconds / dt).round() as usize {
        // (The mouse moves as far a second whatever the frame's length.)
        app.world_mut().resource_mut::<bevy::input::mouse::AccumulatedMouseMotion>().delta = look * dt / DT;
        test_frame(app, dt);
    }
}
fn tap(app: &mut App, key: KeyCode) {
    let dt = app.world().get_resource::<Step>().map_or(DT, |s| s.0);
    hold(app, key, true);
    test_frame(app, dt);
    hold(app, key, false);
}

/// The scripted session (see the module's documentation) at a frame of `dt` seconds; returns the replay's file and the
/// frames drawn.
fn session(dt: f32) -> (std::path::PathBuf, Vec<Drawn>) {
    let mut app = live("sandbox mode=ffa botmode=idle weapons=all");
    app.insert_resource(Step(dt));
    // The pointer captured, as a click does.
    let window = app.world_mut().query_filtered::<Entity, With<Window>>().single(app.world()).unwrap();
    app.world_mut().entity_mut(window).insert(bevy::window::CursorOptions { grab_mode: bevy::window::CursorGrabMode::Locked, ..default() });
    run(&mut app, 0.5, Vec2::ZERO);
    assert!(app.world().resource::<record::Recorder>().recording(), "the round is recorded");
    // Walking and turning.
    hold(&mut app, KeyCode::KeyW, true);
    run(&mut app, 1.2, Vec2::new(14., -2.));
    hold(&mut app, KeyCode::KeyW, false);
    hold(&mut app, KeyCode::KeyD, true);
    run(&mut app, 0.5, Vec2::new(-9., 1.5));
    hold(&mut app, KeyCode::KeyD, false);
    // Every gun in hand in turn, each fired (the mayonnaise squeezed for a while).
    let mut guns = vec![];
    for _ in 0..7 {
        tap(&mut app, KeyCode::KeyY);
        run(&mut app, 0.25, Vec2::ZERO);
        guns.push(app.world().resource::<Game>().weapon);
        button(&mut app, MouseButton::Left, true);
        run(&mut app, 0.3, Vec2::new(3., 0.));
        button(&mut app, MouseButton::Left, false);
        run(&mut app, 0.45, Vec2::ZERO);
    }
    guns.sort_by_key(|w| *w as usize);
    guns.dedup();
    assert_eq!(guns.len(), 7, "every gun in hand: {guns:?}");
    // A reload of the gun in hand, half emptied.
    while app.world().resource::<Game>().weapon != Weapon::Pistol {
        tap(&mut app, KeyCode::KeyY);
    }
    for _ in 0..2 {
        button(&mut app, MouseButton::Left, true);
        run(&mut app, 0.05, Vec2::ZERO);
        button(&mut app, MouseButton::Left, false);
        run(&mut app, 0.4, Vec2::ZERO);
    }
    tap(&mut app, KeyCode::KeyR);
    run(&mut app, 0.3, Vec2::ZERO);
    assert!(app.world().resource::<Game>().mag.reload.is_some(), "reloading");
    run(&mut app, 2.2, Vec2::new(2., 0.));
    // The rifle's optic up, turning, fired through, down; the sniper's scope.
    while app.world().resource::<Game>().weapon != Weapon::Rifle {
        tap(&mut app, KeyCode::KeyY);
    }
    button(&mut app, MouseButton::Right, true);
    run(&mut app, 0.5, Vec2::new(4., 1.));
    assert!(app.world().resource::<Game>().aim > 0.99, "the optic up");
    button(&mut app, MouseButton::Left, true);
    run(&mut app, 0.1, Vec2::ZERO);
    button(&mut app, MouseButton::Left, false);
    run(&mut app, 0.5, Vec2::ZERO);
    button(&mut app, MouseButton::Right, false);
    run(&mut app, 0.4, Vec2::ZERO);
    while app.world().resource::<Game>().weapon != Weapon::Sniper {
        tap(&mut app, KeyCode::KeyY);
    }
    button(&mut app, MouseButton::Right, true);
    run(&mut app, 0.4, Vec2::new(-3., 0.));
    assert!(app.world().resource::<Game>().scoped, "the sniper's scope");
    button(&mut app, MouseButton::Right, false);
    run(&mut app, 0.3, Vec2::ZERO);
    // Hit, from in front and from behind.
    for (from, amount) in [(Vec3::new(0., 0., -1.), 18.), (Vec3::new(0.5, 0., 1.), 22.)] {
        let mut g = app.world_mut().resource_mut::<Game>();
        let time = g.feast.elapsed;
        let me = g.feast.player_mut(1).unwrap();
        let at = Vec3::from_array(me.p) + from;
        me.last_damage = Damage { kind: 1, attacker: 2, p: at.to_array(), d: (-from).normalize().to_array(), time, amount };
        me.hp -= amount;
        drop(g);
        run(&mut app, 0.35, Vec2::new(5., 0.));
    }
    // A kill of this player's (the slow motion, the marker, the cross).
    {
        let mut g = app.world_mut().resource_mut::<Game>();
        let time = g.feast.elapsed;
        let p = g.feast.player_mut(2).unwrap();
        p.last_damage = Damage { kind: 6, attacker: 1, p: [0., 1., 3.], d: [0., 0., -1.], time, amount: p.hp };
        (p.hp, p.alive, p.respawn) = (0., false, 3.);
    }
    run(&mut app, 1.4, Vec2::new(-4., 0.));
    // Death, and back.
    {
        let mut g = app.world_mut().resource_mut::<Game>();
        let time = g.feast.elapsed;
        let me = g.feast.player_mut(1).unwrap();
        me.last_damage = Damage { kind: 1, attacker: 3, p: [0., 1., 0.], d: [0., 0., 1.], time, amount: me.hp };
        (me.hp, me.alive, me.respawn) = (0., false, 1.2);
    }
    run(&mut app, 0.6, Vec2::ZERO);
    assert!(app.world().resource::<Game>().roster.iter().any(|p| p.id == 1 && !p.alive), "dead");
    run(&mut app, 1.6, Vec2::ZERO);
    assert!(app.world().resource::<Game>().roster.iter().any(|p| p.id == 1 && p.alive), "back");
    hold(&mut app, KeyCode::KeyW, true);
    run(&mut app, 0.6, Vec2::new(6., 0.));
    hold(&mut app, KeyCode::KeyW, false);
    let pov = app.world().resource::<Pov>();
    let (batches, bytes, frames) = (pov.batches, pov.bytes, pov.frames);
    app.world_mut().resource_mut::<record::Recorder>().finish();
    let name = app.world().resource::<record::Recorder>().finished.last().cloned().unwrap();
    let path = store::local_dir().unwrap().join(format!("{name}.replay"));
    let truth = std::mem::take(&mut app.world_mut().resource_mut::<Truth>().0);
    let seconds = truth.last().unwrap().elapsed - truth[0].elapsed;
    println!("POV-SIZE single player at {:.0} frames a second: {frames} frames kept in {batches} batches, {bytes} bytes for {seconds:.1} s: {:.0} bytes a minute", 1. / dt, bytes as f32 / seconds * 60.);
    (path, truth)
}

/// The viewer on `path` as main runs it, with the presentation's systems and the recorded view placed once the frame
/// has moved everything else.
fn viewer(path: &std::path::Path) -> App {
    let mut app = test_app("");
    app.add_plugins(bevy::transform::TransformPlugin);
    {
        let mut o = app.world_mut().resource_mut::<crate::runtime::Options>();
        (o.replay, o.demo, o.connect) = (path.to_string_lossy().into(), true, crate::runtime::REPLAY.into());
    }
    app.world_mut().resource_mut::<Game>().demo = true;
    app.world_mut().insert_resource(crate::lobby::Lobby::default());
    app.init_resource::<crate::network::Network>()
        .init_resource::<crate::controls::Bindings>()
        .init_resource::<crate::settings::Settings>()
        .init_resource::<SlowMo>()
        .init_resource::<Hurt>()
        .init_resource::<Shown>()
        .init_resource::<Time<Virtual>>()
        .init_resource::<crate::replay::list::List>()
        .insert_resource(Viewer::new(&path.to_string_lossy()))
        .add_systems(Update, (feed, crate::network::receive, restore, crate::arena::tick, crate::network::send, camera, crate::reload::follow, crate::damage::update, crate::slowmo::present, crate::replay::recorded).chain())
        .add_systems(PostUpdate, crate::replay::recorded.before(bevy::transform::TransformSystems::Propagate));
    app
}
/// The angle between two turns (precise when it is small, as `acos` of their dot product is not).
fn angle(a: Quat, b: Quat) -> f32 {
    let d = (a.inverse() * b).normalize();
    2. * d.xyz().length().atan2(d.w.abs())
}

#[test]
fn a_recorded_view_plays_back_exactly_where_the_game_drew_it_frame_by_frame_and_as_it_looked() {
    let _one = store::FOLDER.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_dir_all(store::local_dir().unwrap());
    let (path, truth) = session(DT);
    let replay = Replay::open(&path).unwrap();
    assert_eq!(replay.header.format, kitchen_sim::replay::FORMAT);
    let offset = replay.povs.get(&1).expect("single player's view is in its replay").offset;
    let track = replay.track(1).unwrap();
    assert_eq!(track.header.clock, "round");
    // Every frame drawn while the round was recorded is in the track, on the replay's clock.
    assert_eq!(track.samples.len(), truth.len(), "a sample a frame (60 a second is under the rate)");
    let (mut kept_pos, mut kept_turn, mut kept_fov) = (0f32, 0f32, 0f32);
    for (s, d) in track.samples.iter().zip(&truth) {
        assert!((s.t - (d.elapsed as f64 * 1000. - offset)).abs() < 0.02, "{} and {}", s.t, d.elapsed);
        kept_pos = kept_pos.max((Vec3::from_array(s.cam) - d.cam.0).abs().max_element());
        kept_turn = kept_turn.max(angle(bevy_quat(kitchen_sim::pov::quat(s.turn)), d.cam.1));
        kept_fov = kept_fov.max((s.fov - d.fov).abs());
    }
    println!("POV-KEPT the track against what was drawn: position {kept_pos:e} m, turn {kept_turn:e} rad, field of view {kept_fov:e} rad at most");
    assert!(kept_pos <= POSITION && kept_turn <= TURN && kept_fov <= POSITION, "{kept_pos} {kept_turn} {kept_fov}");
    // The events the screen had.
    let names: std::collections::BTreeSet<&str> = track.events.iter().map(|e| e.event.name()).collect();
    for want in ["shot", "reload", "weapon", "scope", "hurt", "dealt", "slow"] {
        assert!(names.contains(want), "{want} among {names:?}");
    }
    // Played back, paused at each frame's own moment in turn: the camera exactly where the track has it.
    let mut app = viewer(&path);
    for _ in 0..3 {
        test_frame(&mut app, DT);
    }
    {
        let mut v = app.world_mut().resource_mut::<Viewer>();
        v.paused = true;
        assert_eq!(v.watched, 1);
    }
    let cam = app.world().resource::<Game>().camera().unwrap();
    let (mut exact_pos, mut exact_turn, mut exact_fov) = (0f32, 0f32, 0f32);
    let (mut drawn_pos, mut drawn_turn, mut drawn_fov, mut gun_pos, mut gun_turn, mut mag) = (0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
    let mut compared = [0usize; 4];
    for (i, (s, d)) in track.samples.iter().zip(&truth).enumerate() {
        // (The replay's clock ends at its last frame, in whole ms: a view's last frame may be a fraction past it.)
        if s.t > replay.duration() as f64 {
            continue;
        }
        app.world_mut().resource_mut::<Viewer>().clock = s.t;
        test_frame(&mut app, DT);
        let world = app.world_mut();
        assert!(world.resource::<Viewer>().recorded(), "through the recorded view at {} ms", s.t);
        let (_, turn, at) = world.get::<GlobalTransform>(cam).unwrap().to_scale_rotation_translation();
        let Projection::Perspective(p) = world.get::<Projection>(cam).unwrap().clone() else { panic!() };
        exact_pos = exact_pos.max((at - Vec3::from_array(s.cam)).abs().max_element());
        exact_turn = exact_turn.max(angle(turn, bevy_quat(kitchen_sim::pov::quat(s.turn))));
        exact_fov = exact_fov.max((p.fov - s.fov).abs());
        drawn_pos = drawn_pos.max((at - d.cam.0).abs().max_element());
        drawn_turn = drawn_turn.max(angle(turn, d.cam.1));
        drawn_fov = drawn_fov.max((p.fov - d.fov).abs());
        // What the events and the state drive, at every tenth frame and wherever something shows.
        let busy = d.marker.is_some() || d.arcs > 0 || d.scoped || d.slow > 0. || d.magazine.is_some_and(|m| m != Transform::IDENTITY);
        if i % 10 != 0 && !busy {
            continue;
        }
        let shown = drawn(world).unwrap();
        compared[0] += 1;
        assert_eq!(shown.gun.map(|g| g.0), d.gun.map(|g| g.0), "the gun in hand at {} ms", s.t);
        if let (Some(a), Some(b)) = (shown.gun, d.gun) {
            gun_pos = gun_pos.max((a.1 - b.1).abs().max_element());
            gun_turn = gun_turn.max(angle(a.2, b.2));
        }
        if let (Some(a), Some(b)) = (shown.magazine, d.magazine) {
            mag = mag.max((a.translation - b.translation).abs().max_element());
        }
        assert_eq!((shown.scoped, shown.weapon), (d.scoped, d.weapon), "the scope at {} ms", s.t);
        assert!((shown.aim - d.aim).abs() < 1e-3, "the optic {} and {} at {} ms", shown.aim, d.aim, s.t);
        assert_eq!(shown.marker.map(|m| m.1), d.marker.map(|m| m.1), "the hit marker at {} ms", s.t);
        if let (Some(a), Some(b)) = (shown.marker, d.marker) {
            assert!((a.0 - b.0).abs() < 1e-3, "the marker's moment {} and {}", a.0, b.0);
            compared[1] += 1;
        }
        assert_eq!(shown.arcs, d.arcs, "the damage indicators at {} ms", s.t);
        compared[2] += (d.arcs > 0) as usize;
        assert_eq!(shown.crosshair, d.crosshair, "the crosshair at {} ms", s.t);
        let slow = world.resource::<SlowMo>().level(crate::replay::pov::now(world));
        assert!((slow - d.slow).abs() < 1e-3, "the slow motion {slow} and {} at {} ms", d.slow, s.t);
        compared[3] += (d.slow > 0.) as usize;
    }
    println!("POV-EXACT the camera played against the track: position {exact_pos:e} m, turn {exact_turn:e} rad, field of view {exact_fov:e} rad at most, over {} frames", track.samples.len());
    println!("POV-DRAWN the camera played against what the game drew: position {drawn_pos:e} m, turn {drawn_turn:e} rad, field of view {drawn_fov:e} rad; the gun {gun_pos:e} m, {gun_turn:e} rad; the magazine {mag:e} m");
    println!("POV-STATES compared at {} frames: {} with the hit marker up, {} with indicators, {} in slow motion", compared[0], compared[1], compared[2], compared[3]);
    assert!(exact_pos <= 1e-6 && exact_turn <= 2e-6 && exact_fov == 0., "within float precision: {exact_pos} {exact_turn} {exact_fov}");
    assert!(drawn_pos <= POSITION && drawn_turn <= TURN * 1.5 && drawn_fov <= POSITION);
    assert!(gun_pos <= 0.5 / 262_144. + 2e-6 && gun_turn <= TURN * 1.5 && mag < 1e-4, "{gun_pos} {gun_turn} {mag}");
    assert!(compared[1] > 5 && compared[2] > 5 && compared[3] > 5, "{compared:?}");

    // A seek to just after the kill: the marker, the indicators and the slow motion as the frames played up to there had
    // them (what came before is heard again, each at its own moment).
    let kill = track.events.iter().find(|e| matches!(e.event, kitchen_sim::pov::Event::Dealt { kill: true, .. })).unwrap();
    let after = (kill.at + 12).min(track.samples.len() - 1);
    let at = track.samples[after].t;
    app.world_mut().resource_mut::<Viewer>().seek = Some(at);
    test_frame(&mut app, 0.2);
    app.world_mut().resource_mut::<Viewer>().clock = at;
    test_frame(&mut app, DT);
    let shown = drawn(app.world_mut()).unwrap();
    let d = &truth[after];
    assert!(app.world().resource::<Viewer>().seeks >= 2);
    assert_eq!((shown.marker.map(|m| m.1), shown.arcs, shown.crosshair.clone()), (d.marker.map(|m| m.1), d.arcs, d.crosshair.clone()), "after a seek");
    let (_, turn, at3) = app.world().get::<GlobalTransform>(cam).unwrap().to_scale_rotation_translation();
    assert!((at3 - Vec3::from_array(track.samples[after].cam)).abs().max_element() < 1e-6 && angle(turn, bevy_quat(kitchen_sim::pov::quat(track.samples[after].turn))) < 2e-6);
    let slow = app.world().resource::<SlowMo>().level(crate::replay::pov::now(app.world()));
    assert!((slow - d.slow).abs() < 1e-3, "the slow motion after a seek: {slow} and {}", d.slow);

    // Played at 0.25x, 2x and at another frame rate: the camera is the track's view at the clock, between its frames.
    for (speed, dt) in [(0usize, 1. / 37.), (3, 1. / 144.), (2, 1. / 50.)] {
        {
            let mut v = app.world_mut().resource_mut::<Viewer>();
            (v.paused, v.speed, v.clock) = (false, speed, 2000.);
        }
        for _ in 0..40 {
            test_frame(&mut app, dt);
            let clock = app.world().resource::<Viewer>().clock;
            let Some(want) = track.view(clock) else { continue };
            let (_, turn, at) = app.world().get::<GlobalTransform>(cam).unwrap().to_scale_rotation_translation();
            assert!((at - Vec3::from_array(want.cam)).abs().max_element() < 1e-6, "at {clock} ms, {speed}");
            assert!(angle(turn, bevy_quat(kitchen_sim::pov::quat(want.turn))) < 2e-6);
        }
    }
    let _ = std::fs::remove_dir_all(store::local_dir().unwrap());
}

/// Online: the client sends its view to the server as `{"type":"pov"}` texts beside its commands (only once welcomed),
/// on the server's clock; kept as the server keeps them (crates/server replay.rs: checked, then a header and the
/// batches in the match's recording, its start on that clock) the match's replay plays through the view exactly. The
/// client's own copy beside its replays is the same view, and a recording of the match without it is watched through
/// it too.
#[test]
fn online_the_view_goes_to_the_server_and_its_recording_plays_through_it() {
    use crate::network::{Incoming, Outgoing};
    use kitchen_sim::{net, pov as track, replay as file};
    let _one = store::FOLDER.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_dir_all(store::local_dir().unwrap());
    let (mut app, server, sent) = crate::network::tests::connected();
    app.add_plugins(bevy::transform::TransformPlugin);
    {
        let mut o = app.world_mut().resource_mut::<crate::runtime::Options>();
        (o.pov, o.record) = (true, true);
    }
    app.init_resource::<crate::controls::Bindings>()
        .init_resource::<crate::settings::Settings>()
        .init_resource::<Pov>()
        .init_resource::<Shown>()
        .init_resource::<Hurt>()
        .init_resource::<SlowMo>()
        .add_systems(Update, (crate::network::receive, crate::arena::tick, crate::damage::update, crate::network::send).chain())
        .add_systems(Last, sample);
    let window = app.world_mut().query_filtered::<Entity, With<Window>>().single(app.world()).unwrap();
    app.world_mut().entity_mut(window).insert(bevy::window::CursorOptions { grab_mode: bevy::window::CursorGrabMode::Locked, ..default() });
    // The server: a round on the bundled map, hotdog 2 this client's; it began at step 600 and records from step 900.
    let map = kitchen_sim::map::Map::bundled();
    let mut feast = kitchen_sim::feast::Feast::with_map(map.clone(), false);
    let (hz, first, start) = (60u32, 600u32, 900u32);
    let first_frame = file::snapshot(&feast, Some(2), |_| String::new());
    let mut header = file::Header::new("hotdog-server test", "server", &map, "siege", "", file::roster(&first_frame), store::now());
    header.start_ms = start as f64 * 1000. / hz as f64;
    let mut recording = file::Writer::new(Vec::new(), &header).unwrap();
    let mut quiet = file::Writer::new(Vec::new(), &header).unwrap();
    let (mut said, mut views, mut texts_before_welcome) = (None::<String>, 0, 0);
    let mut welcomed = false;
    hold(&mut app, KeyCode::KeyW, true);
    for tick in first..first + 60 * 12 {
        feast.tick(1. / hz as f32);
        if tick % 2 == 0 {
            let frame = file::snapshot(&feast, Some(2), |_| String::new());
            let state = frame.state();
            let bytes = net::encode_frame(&net::Header { tick, base: 0, ack: 0, checksum: state.checksum(), elapsed: frame.elapsed, mine: None }, None, &state, &net::Events::default());
            server.send(Incoming::Binary(bytes)).unwrap();
            welcomed = true;
            if tick >= start {
                let ms = (tick - start) * 1000 / hz;
                recording.frame(ms, tick, &frame, &state).unwrap();
                quiet.frame(ms, tick, &frame, &state).unwrap();
            }
        }
        app.world_mut().resource_mut::<bevy::input::mouse::AccumulatedMouseMotion>().delta = Vec2::new(7., (tick as f32 * 0.1).sin() * 3.);
        test_frame(&mut app, 1. / hz as f32);
        // What the client sent: its views kept as the server keeps them, once the match is being recorded.
        while let Ok(message) = sent.try_recv() {
            let Outgoing::Text(text) = message else { continue };
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            if v["type"] != "pov" {
                continue;
            }
            texts_before_welcome += !welcomed as usize;
            if let Some(h) = v.get("header") {
                let h = track::Header::parse(&h.to_string()).unwrap();
                assert_eq!((h.player, h.clock.as_str(), h.source.as_str()), (2, "server", "online"));
                said = Some(h.text());
                continue;
            }
            let bytes = track::unbase64(v["data"].as_str().unwrap()).unwrap();
            // (Before the recording begins the server keeps nothing: it passes them over.)
            if tick < start {
                continue;
            }
            let now = tick as f64 * 1000. / hz as f64;
            track::check(&bytes, header.start_ms - 5000., now + 2000.).expect("the server takes it");
            {
                if views == 0 {
                    recording.pov_header(2, header.start_ms, said.as_ref().expect("a header first")).unwrap();
                }
                recording.pov(2, &bytes).unwrap();
                views += 1;
            }
        }
    }
    hold(&mut app, KeyCode::KeyW, false);
    assert_eq!(texts_before_welcome, 0, "nothing is sent before the server has spoken");
    assert!(views >= 5, "{views} batches kept");
    let pov = app.world().resource::<Pov>();
    println!("POV-SIZE online: {} frames in {} batches, {} bytes for {:.1} s: {:.0} bytes a minute", pov.frames, pov.batches, pov.bytes, pov.span.map_or(0., |(a, b)| (b - a) / 1000.), pov.bytes as f64 / pov.span.map_or(1., |(a, b)| (b - a) / 60_000.));
    // The client leaves: its own copy is finished.
    app.world_mut().remove_resource::<Pov>();
    let dir = store::local_dir().unwrap();
    let path = dir.join("online.replay");
    std::fs::write(&path, recording.finish(1_800_000_010).unwrap().0).unwrap();
    let replay = Replay::open(&path).unwrap();
    let view = replay.track(2).expect("the client's view is in the match's recording");
    let (a, b) = view.span().unwrap();
    assert!(a >= -5000. && b <= replay.duration() as f64 + 200. && b - a > 3000., "{a}..{b} of {}", replay.duration());
    // Watched: through it, the camera exactly where the track has it.
    let mut viewer = viewer(&path);
    for _ in 0..3 {
        test_frame(&mut viewer, DT);
    }
    assert_eq!(viewer.world().resource::<Viewer>().watched, 2, "the player with a view is watched first");
    viewer.world_mut().resource_mut::<Viewer>().paused = true;
    let cam = viewer.world().resource::<Game>().camera().unwrap();
    let mut played = 0;
    for s in view.samples.iter().filter(|s| s.t >= 0. && s.t <= replay.duration() as f64).step_by(7) {
        viewer.world_mut().resource_mut::<Viewer>().clock = s.t;
        test_frame(&mut viewer, DT);
        assert!(viewer.world().resource::<Viewer>().recorded());
        let (_, turn, at) = viewer.world().get::<GlobalTransform>(cam).unwrap().to_scale_rotation_translation();
        assert!((at - Vec3::from_array(s.cam)).abs().max_element() < 1e-6 && angle(turn, bevy_quat(kitchen_sim::pov::quat(s.turn))) < 2e-6, "at {} ms", s.t);
        played += 1;
    }
    assert!(played > 20);
    // The client's own copy: a view on the server's clock, beside its replays, the same frames.
    let copies: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| store::view_only(&p.to_string_lossy())).collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    let copy = Replay::open(&copies[0]).unwrap();
    assert!(copy.chunks.is_empty() && copy.header.source == "view");
    let mine = copy.track(2).unwrap();
    let shared: Vec<&kitchen_sim::pov::Sample> = view.samples.iter().collect();
    let same = mine.samples.iter().filter(|s| shared.iter().any(|v| (v.t - (s.t - header.start_ms)).abs() < 1e-6 && v.cam == s.cam)).count();
    assert!(same >= shared.len() - 1, "{same} of {} frames alike", shared.len());
    assert!(store::list_local().iter().all(|e| !store::view_only(&e.key)), "the list leaves the copy out");
    // A recording of the match without the view (a server that did not take it): watched through the client's own copy.
    let quiet_path = dir.join("quiet.replay");
    std::fs::write(&quiet_path, quiet.finish(1_800_000_010).unwrap().0).unwrap();
    let quiet = Replay::open(&quiet_path).unwrap();
    assert!(quiet.povs.is_empty());
    let mut views = super::Views::of(&quiet);
    assert!(views.has(&quiet, 2), "the copy kept here is found for it");
    let s = &view.samples[view.samples.len() / 2];
    let found = views.at(&quiet, 2, s.t).unwrap();
    assert!((found.t - s.t).abs() < 1e-6 && found.cam == s.cam);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The setting and the flags: off, no view is sent (and none kept with `--no-pov`); the switch is kept; nothing of it
/// is written beyond the tests' own folder.
#[test]
fn the_switch_and_the_flags_keep_the_view_from_the_server_or_from_anywhere() {
    let mut options = crate::runtime::Options::default();
    assert!(!options.pov && !options.record, "tests keep nothing unless they ask");
    options.pov = true;
    assert!(allowed(&options));
    assert!(!allowed(&crate::runtime::Options { replay: "x.replay".into(), ..options.clone() }), "never in the viewer");
    assert!(!allowed(&crate::runtime::Options { demo: true, ..options.clone() }), "never in the demo");
    // The switch, kept in the preferences (in memory in tests: prefs.rs).
    let mut prefs = crate::prefs::Prefs::default();
    let mut p = Pov::default();
    assert!(p.share, "on to begin with");
    p.set(Some(&mut prefs), false);
    assert_eq!((p.share, prefs.get(KEY).as_deref()), (false, Some("off")));
    // Off: the batches are kept (the copy) but nothing goes to the server.
    (p.told, p.header) = (Some(2), Some(kitchen_sim::pov::Header { player: 2, clock: "server".into(), ..Default::default() }));
    p.batch.samples.push(kitchen_sim::pov::Sample { t: 1., ..Default::default() });
    p.flush(false);
    assert!(p.outbox.is_empty() && p.batches == 1);
    // Turned on in the middle of a match: the header goes first, then the batches.
    p.set(None, true);
    for t in [2., 3.] {
        p.batch.samples.push(kitchen_sim::pov::Sample { t, ..Default::default() });
        p.flush(false);
    }
    let sent: Vec<serde_json::Value> = p.outbox.iter().map(|t| serde_json::from_str(t).unwrap()).collect();
    assert_eq!(sent.len(), 3);
    assert!(sent[0].get("header").is_some() && sent[1].get("data").is_some() && sent[2].get("data").is_some());
    // The tests' folder is beside the test binary, never the player's own.
    let dir = store::local_dir().unwrap();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    assert!(dir.starts_with(std::env::current_exe().unwrap().parent().unwrap()) && (home.as_os_str().is_empty() || !dir.starts_with(home.join(".config"))));
}

/// Watched without a recorded view (a bot, an older file): RECONSTRUCTED, with the gun in hand, kicked by the shots the
/// frames tell of; an older file (format 1) plays so.
#[test]
fn without_a_recorded_view_the_reconstruction_has_the_gun_in_hand_kicked_by_its_shots_and_an_old_file_plays() {
    // Free For All's bots fight: their shots are in the frames.
    let (bytes, _) = crate::replay::tests::recorded(10.);
    let mut old = bytes.clone();
    old[8..12].copy_from_slice(&1u32.to_le_bytes());
    let path = std::env::current_exe().unwrap().with_file_name(format!("pov-test-old-{}.replay", std::process::id()));
    std::fs::write(&path, &old).unwrap();
    let mut app = viewer(&path);
    let mut kicked = false;
    let mut shown = 0;
    for _ in 0..500 {
        test_frame(&mut app, DT);
        let v = app.world().resource::<Viewer>();
        assert!(v.failed.is_none(), "{:?}", v.failed);
        assert!(!v.recorded(), "no view in an old file");
        let g = app.world().resource::<Game>();
        let shown_gun = g.viewmodels().iter().find(|(_, e)| app.world().get::<Visibility>(*e) == Some(&Visibility::Visible));
        if let Some((w, e)) = shown_gun {
            shown += 1;
            assert_eq!(*w, g.weapon, "the watched player's gun");
            let t = app.world().get::<Transform>(*e).unwrap();
            kicked |= t.translation.z > -0.105 + 0.003;
        }
    }
    assert!(shown > 100, "the gun in hand ({shown} frames)");
    assert!(kicked, "kicked by a shot");
    let _ = std::fs::remove_file(path);
}

/// Bytes a minute of the scripted session's view at 30, 60, 144 and 240 frames a second (at most RATE are kept).
#[test]
#[ignore]
fn sizes() {
    let _one = store::FOLDER.lock().unwrap_or_else(|e| e.into_inner());
    for hz in [30., 60., 144., 240.] {
        let _ = std::fs::remove_dir_all(store::local_dir().unwrap());
        let (path, _) = session(1. / hz);
        let replay = Replay::open(&path).unwrap();
        let track = replay.track(1).unwrap();
        let (a, b) = track.span().unwrap();
        let kept: usize = replay.bytes().len();
        println!("POV-SIZES {hz} frames a second: {} frames kept over {:.1} s; the replay file {kept} bytes in all", track.samples.len(), (b - a) / 1000.);
    }
    let _ = std::fs::remove_dir_all(store::local_dir().unwrap());
}

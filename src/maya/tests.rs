// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
use super::act::{self, Steps};
use super::choreography::{self as c, Beat};
use super::Stage;
use crate::human::{self, Grasp, Person, Viseme};
use bevy::prelude::*;

#[test]
fn the_scene_keeps_the_clock_it_always_had() {
    // The beats' times, as the breakfast scene was first made (tools/maya_choreography.py, now this module).
    for (value, was) in [
        (c::FPS, 30.),
        (c::GREETING_START, 5.6),
        (c::GREETING_DURATION, 11.),
        (c::IDLE_START, 16.6),
        (c::IDLE_DURATION, 4.5),
        (c::REACH_START, 21.1),
        (c::REACH_DURATION, 4.),
        (c::CARRY_START, 25.1),
        (c::CARRY_DURATION, 8.7),
        (c::SET_START, 33.8),
        (c::SET_DURATION, 4.),
        (c::COMPLETE_START, 37.8),
        (c::KNIFE_START, 42.3),
    ] {
        assert_eq!(value, was);
    }
    // The stages, played at 60 frames a second with Space 2.6 s into the wait, change when they always did.
    let (mut stage, mut entered, mut changes) = (Stage::Opening, 0., vec![]);
    for frame in 0..60 * 60 {
        let now = frame as f32 / 60.;
        let called = stage == Stage::Waiting && now - entered >= 2.6 && now - entered < 2.6 + 1. / 60.;
        if let Some(next) = stage.next(now - entered, now, called) {
            (stage, entered) = (next, now);
            changes.push((next, now));
        }
    }
    let expected = [
        (Stage::Greeting, 5.6),
        (Stage::Waiting, 16.6),
        (Stage::Reaching, 19.2),
        (Stage::Carrying, 23.2),
        (Stage::SettingDown, 31.9),
        (Stage::Complete, 35.9),
    ];
    assert_eq!(changes.len(), expected.len(), "{changes:?}");
    for ((stage, at), (want, when)) in changes.iter().zip(expected) {
        assert_eq!(*stage, want);
        assert!((at - when).abs() < 0.02, "{stage:?} at {at}, not {when}");
    }
    // Each stage plays its stretch of the clock; the waits loop.
    assert_eq!(Stage::Greeting.clip_time(3., 9.), c::GREETING_START + 3.);
    assert_eq!(Stage::Waiting.clip_time(5., 30.), c::IDLE_START + 0.5);
    assert_eq!(Stage::Carrying.clip_time(20., 50.), c::CARRY_START + c::CARRY_DURATION);
}

#[test]
fn the_tray_and_the_hotdogs_eye_follow_the_paths_they_always_did() {
    // Frames of the table the scene used to be drawn from (package xyz, Maya's root xyz, her yaw, the package's yaw;
    // then the camera's eye xyz, target xyz and field of view), every number as it was.
    let rows: [(usize, [f32; 8], [f32; 7]); 14] = [
        (170, [0.0, 0.9525, -0.36, 0.0, 0.0, 1.02, -0.01317, 0.0], [0.0, 0.0325, 0.0, 0.00204, 0.2049, 1.35092, 81.68948]),
        (200, [0.0, 0.9525, -0.36, 0.0, 0.0, 1.02, -1.71385, 0.0], [0.0, 0.0325, 0.0, 0.28324, 0.19104, 1.47766, 38.91615]),
        (260, [0.0, 0.9525, -0.36, 0.0, 0.0, 1.02, -2.0, 0.0], [0.0, 0.0325, 0.0, 0.355, 0.1875, 1.51, 28.0]),
        (320, [0.0, 0.9525, -0.36, 0.17771, 0.0, 1.06936, -2.0, 0.0], [0.0, 0.0325, 0.0, 0.39229, 0.40059, 1.51, 52.86053]),
        (400, [0.0, 0.9525, -0.36, 0.16017, 0.0, 1.06449, -1.94242, 0.0], [0.03463, 0.17844, -0.1682, 0.02102, 0.02233, 0.05554, 48.08459]),
        (640, [0.0, 0.9525, -0.36, 0.0, 0.0, 0.99178, 0.0, 0.0], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (760, [0.0, 0.9525, -0.34087, 0.0, 0.0, 0.13913, 0.0, 0.0], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (800, [0.0, 0.9525, 0.12745, 0.0, 0.0, 0.60745, 0.0, 0.0], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (850, [-0.00143, 1.08, 0.55, 0.0, 0.0, 1.03, 0.00297, 0.00297], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (960, [0.62145, 1.08, 2.27291, 0.40679, 0.0, 1.84358, 3.60524, 3.60524], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (1050, [0.85, 1.00925, 3.23, 0.85, 0.0, 2.73, 3.14159, 3.14159], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (1100, [0.85, 0.9385, 3.25, 0.85, 0.0, 2.71512, 3.14159, 3.14159], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (1300, [0.85, 0.9385, 3.25, 0.85, 0.0, 2.66565, 3.14159, 3.14159], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
        (1629, [0.85, 0.9385, 3.25, 0.85, 0.0, 2.72, 3.14159, 3.14159], [0.0, 0.0325, 0.0, 0.0, 0.205, 1.35, 82.0]),
    ];
    for (frame, pose, camera) in rows {
        let t = frame as f32 / c::FPS;
        let m = c::motion(t);
        let mine = [m.package.x, m.package.y, m.package.z, m.root.x, m.root.y, m.root.z, m.yaw, m.package_yaw];
        for (a, b) in mine.iter().zip(pose) {
            assert!((a - b).abs() < 2e-4, "frame {frame}: {mine:?} against {pose:?}");
        }
        let k = c::camera(t);
        for (a, b) in k.iter().zip(camera) {
            assert!((a - b).abs() < 2e-3, "frame {frame}: camera {k:?} against {camera:?}");
        }
    }
}

/// The world's place of the point a person's hand holds at clip time `t` (side 0 left, 1 right).
fn held(actor: &act::Actor, person: Person, side: usize) -> (Vec3, f32) {
    let solved = human::solve(person, &actor.pose);
    let hand = actor.place(1.) * solved.hands[side];
    (hand.transform_point(human::grip_point(side, human::hand_scale(person))), solved.short[side])
}

#[test]
fn hands_that_hold_something_hold_it() {
    let steps = Steps::plan();
    let at = |t: f32| act::act(t, &steps);
    // The tray, by its two rims, from the moment the reach closes on it until it is let go on the counter.
    let mut t = c::REACH_START + 4.;
    while t < c::SET_START + 2.4 {
        let [maya, _] = at(t);
        let package = act::package(t);
        for side in 0..2 {
            let sign = if side == 0 { -1. } else { 1. };
            let rim = package.transform_point(Vec3::new(sign * 0.113, 0., 0.03));
            let (point, short) = held(&maya, Person::Maya, side);
            assert!(maya.pose.hands[side].grasp == Grasp::Grip, "t {t}");
            assert!(short < 0.005 && point.distance(rim) < 0.01, "t {t}, side {side}: holds {point}, the rim is at {rim} ({short} short)");
        }
        t += 0.05;
    }
    // The knife, by its handle, from when she has it until the seal is open.
    let mut s = 2.;
    while s < 12. {
        let [maya, _] = at(c::KNIFE_START + s);
        let handle = c::knife(s).transform_point(c::KNIFE_HANDLE);
        let (point, short) = held(&maya, Person::Maya, 1);
        assert!(short < 0.005 && point.distance(handle) < 0.01, "knife at {s}: {point} against {handle}");
        s += 0.05;
    }
    // The right hands meet in the handshake: palm to palm, a hand's thickness apart at most, never through each other.
    let mut s = 2.7;
    while s < 7.3 {
        let t = c::GREETING_START + s;
        let [maya, husband] = at(t);
        let centre = c::greeting(t).centre;
        for (actor, person) in [(maya, Person::Maya), (husband, Person::Husband)] {
            let solved = human::solve(person, &actor.pose);
            let hand = actor.place(1.) * solved.hands[1];
            let palm = hand.transform_point(Vec3::new(-0.022, -0.052, 0.) * human::hand_scale(person));
            assert!(solved.short[1] < 0.005 && palm.distance(centre) < 0.012, "{person:?} at {s}: palm {palm}, centre {centre}");
        }
        s += 0.1;
    }
}

#[test]
fn feet_stay_on_the_floor_and_take_steps() {
    let steps = Steps::plan();
    let mut moved = [0f32; 2];
    let mut last: Option<[Vec3; 4]> = None;
    let mut t = 0.;
    while t < c::END {
        let actors = act::act(t, &steps);
        let mut feet = [Vec3::ZERO; 4];
        for (k, (actor, person)) in actors.iter().zip(Person::ALL).enumerate() {
            let solved = human::solve(person, &actor.pose);
            for side in 0..2 {
                assert!(solved.short[2 + side] < 0.01, "{person:?} at {t}: a foot {} short", solved.short[2 + side]);
                let ankle = actor.place(1.).transform_point(solved.feet[side].translation);
                assert!(ankle.y > person.figure().ankle - 0.002 && ankle.y < person.figure().ankle + 0.05, "{person:?} at {t}: ankle at {ankle}");
                feet[k * 2 + side] = ankle;
            }
        }
        if let Some(before) = last {
            for k in 0..2 {
                moved[k] += before[k * 2].distance(feet[k * 2]) + before[k * 2 + 1].distance(feet[k * 2 + 1]);
            }
        }
        last = Some(feet);
        t += 1. / 60.;
    }
    // Maya walks to the counter and back to it with the knife; he shuffles round to her and back.
    assert!(moved[0] > 4., "Maya's feet moved {} m", moved[0]);
    assert!(moved[1] > 0.1 && moved[1] < 2., "his feet moved {} m", moved[1]);
}

#[test]
fn they_say_their_lines() {
    let line = "Right. Buns are ready.";
    let shapes: Vec<Viseme> = (0..200).filter_map(|i| act::say(line, i as f32 * 0.01)).collect();
    assert!(shapes.contains(&Viseme::Ah) && shapes.contains(&Viseme::Ee) && shapes.contains(&Viseme::Mm) && shapes.contains(&Viseme::Oh));
    assert_eq!(act::say(line, -0.1), None);
    assert_eq!(act::say(line, 10.), None);
    // Maya speaks while she reaches, and as she takes the knife; he greets her; and their mouths are still otherwise.
    let steps = Steps::plan();
    let talking = |person: Person, from: f32, to: f32| {
        let k = if person == Person::Maya { 0 } else { 1 };
        (0..((to - from) * 60.) as usize).filter(|i| !matches!(act::act(from + *i as f32 / 60., &steps)[k].pose.mouth, Viseme::Rest | Viseme::Laugh | Viseme::Mm)).count()
    };
    assert!(talking(Person::Maya, c::REACH_START, c::REACH_START + c::REACH_DURATION) > 60);
    assert!(talking(Person::Maya, c::KNIFE_START, c::KNIFE_START + 4.5) > 60);
    assert!(talking(Person::Husband, c::GREETING_START, c::GREETING_START + 2.) > 30);
    assert_eq!(talking(Person::Maya, 0., c::GREETING_START), 0);
    assert_eq!(talking(Person::Husband, c::IDLE_START, c::REACH_START), 0);
}

#[test]
fn the_beats_happen_in_order_and_the_couple_greet() {
    assert_eq!(c::beat(1.).0, Beat::Idle);
    assert_eq!(c::beat(c::GREETING_START + 1.).0, Beat::Greet);
    assert_eq!(c::beat(c::IDLE_START + 1.).0, Beat::Idle);
    assert_eq!(c::beat(c::REACH_START + 1.).0, Beat::Reach);
    assert_eq!(c::beat(c::CARRY_START + 1.).0, Beat::Carry);
    assert_eq!(c::beat(c::SET_START + 1.).0, Beat::Set);
    assert_eq!(c::beat(c::COMPLETE_START + 1.).0, Beat::Done);
    assert_eq!(c::beat(c::KNIFE_START + 1.).0, Beat::Knife);
    let greeting = c::greeting(c::GREETING_START + 6.);
    assert!(greeting.hug > 0.99 && greeting.shake > 0.99);
    // They face each other for it, and face the fridge again after.
    let steps = Steps::plan();
    let [maya, husband] = act::act(c::GREETING_START + 6., &steps);
    let ahead = |a: &act::Actor| Quat::from_rotation_y(a.yaw) * Vec3::NEG_Z;
    let between = (husband.root - maya.root).normalize();
    assert!(ahead(&maya).dot(between) > 0.95 && ahead(&husband).dot(-between) > 0.9);
    // And they look at each other.
    assert!(maya.pose.look.unwrap().z < 0. && husband.pose.look.unwrap().z < 0.);
}

/// The files the intro's old characters came from, which are gone (THIRD_PARTY_NOTICES.md).
const REMOVED: [&str; 9] = [
    "maya.glb",
    "giant.glb",
    "giant_expressive.glb",
    "reconstructed/",
    "person.png",
    "face_skin.wgsl",
    "mediapipe",
    "renderpeople",
    "face_calibration",
];

#[test]
fn nothing_asks_for_a_removed_file() {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut sources = vec![];
    let mut stack = vec![std::path::PathBuf::from(root).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs" || e == "wgsl") && !path.ends_with("maya/tests.rs") {
                sources.push(path);
            }
        }
    }
    for page in ["index.html", "lobby.js", "dev.html", "devlink.js", "crates/server/src/statics.rs"] {
        sources.push(std::path::PathBuf::from(root).join(page));
    }
    assert!(sources.len() > 50);
    for path in sources {
        let text = std::fs::read_to_string(&path).unwrap().to_lowercase();
        for name in REMOVED {
            assert!(!text.contains(name), "{} still names {name}", path.display());
        }
    }
    for name in REMOVED.iter().filter(|n| n.contains('.') || n.ends_with('/')) {
        assert!(!std::path::Path::new(root).join("assets").join(name.trim_end_matches('/')).exists(), "assets/{name} is still there");
    }
}

#[cfg(unix)]
#[test]
fn the_packages_carry_no_removed_file() {
    let root = env!("CARGO_MANIFEST_DIR");
    for kind in ["native", "web"] {
        let out = std::process::Command::new("bash")
            .current_dir(root)
            .env("HOTDOG_VERSION", "0.0.0-test")
            .args(["-c", &format!(". tools/dist-common.sh && dist_asset_list {kind}")])
            .output()
            .expect("bash runs tools/dist-common.sh");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let list = String::from_utf8(out.stdout).unwrap();
        let files: Vec<&str> = list.lines().collect();
        assert!(files.len() > 40, "{kind}: {files:?}");
        assert!(files.iter().any(|f| f.starts_with("audio/")) && files.iter().any(|f| f.starts_with("maps/")));
        for file in &files {
            let lower = file.to_lowercase();
            assert!(REMOVED.iter().all(|name| !lower.contains(name)), "{kind} packages {file}");
            assert!(!lower.ends_with(".mp3"), "{kind} packages an original download, {file}");
            assert!(std::path::Path::new(root).join("assets").join(file).is_file(), "{kind} lists {file}, which is not there");
        }
    }
}

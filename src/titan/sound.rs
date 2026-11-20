// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The titan's sounds, synthesised by tools/build_titan_sounds.py into assets/audio/titan-*.wav: its footfalls (deep
//! booms), its breathing (a slow wet rumble), the roar of steam that warns of a charge, the smash, the whoosh of a swat,
//! the groan when it is brought to its knees, the hiss of its weak spot hit, and the long topple when it falls. Each is
//! played louder the nearer the listener (this screen's camera) is, and quietly from across the room; the levels are
//! kept conservative (nobody has listened to them: see the script).
use bevy::audio::Volume;
use bevy::prelude::*;

/// What is heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Footfall,
    Breath,
    Roar,
    Smash,
    Land,
    Swat,
    Stomp,
    Thump,
    Groan,
    Hurt,
    Topple,
}
/// Each cue's file, its loudest (linear), its playback speed, and how far off (metres) it is still at about half.
fn sheet(cue: Cue) -> (&'static str, f32, f32, f32) {
    match cue {
        Cue::Footfall => ("audio/titan-step.wav", 0.55, 1., 1.6),
        Cue::Breath => ("audio/titan-breath.wav", 0.32, 1., 1.0),
        Cue::Roar => ("audio/titan-roar.wav", 0.6, 1., 3.0),
        Cue::Smash => ("audio/titan-smash.wav", 0.7, 1., 3.0),
        Cue::Land => ("audio/titan-smash.wav", 0.5, 1.15, 2.5),
        Cue::Swat => ("audio/titan-swat.wav", 0.5, 1., 1.6),
        Cue::Stomp => ("audio/titan-step.wav", 0.65, 0.85, 2.0),
        Cue::Thump => ("audio/thwack.wav", 0.4, 0.7, 1.2),
        Cue::Groan => ("audio/titan-groan.wav", 0.6, 1., 3.0),
        Cue::Hurt => ("audio/titan-hiss.wav", 0.55, 1., 3.0),
        Cue::Topple => ("audio/titan-topple.wav", 0.75, 1., 5.0),
    }
}
/// How loud `cue` is heard `distance` from it, at `strength` (1 the usual), and the speed it is played at.
pub fn heard(cue: Cue, distance: f32, strength: f32) -> (f32, f32) {
    let (_, loudest, speed, half) = sheet(cue);
    let near = 1. / (1. + (distance / half).powi(2));
    let volume = loudest * strength.clamp(0.2, 1.5).sqrt() * (0.15 + 0.85 * near);
    // A heavier footfall is a little deeper.
    let speed = speed * (1.08 - 0.1 * strength.clamp(0., 1.5));
    (volume.min(0.85), speed)
}
/// The sounds' handles, loaded the first time one is wanted.
#[derive(Resource, Default)]
pub struct Sounds {
    loaded: Vec<(&'static str, Handle<AudioSource>)>,
    /// When the titan last breathed.
    breath: f32,
}
/// Plays `cue` as heard from where this screen's camera is, `at` being where it happens.
pub fn play(world: &mut World, cue: Cue, at: Vec3, strength: f32) {
    let Some(server) = world.get_resource::<AssetServer>().cloned() else {
        return;
    };
    let ear = world.get_resource::<crate::arena::Game>().and_then(|g| g.camera()).and_then(|c| world.get::<Transform>(c)).map_or(at, |t| t.translation);
    let (volume, speed) = heard(cue, ear.distance(at), strength);
    if volume < 0.01 {
        return;
    }
    let (path, ..) = sheet(cue);
    if !world.contains_resource::<Sounds>() {
        world.init_resource::<Sounds>();
    }
    let handle = {
        let mut sounds = world.resource_mut::<Sounds>();
        match sounds.loaded.iter().find(|(p, _)| *p == path) {
            Some((_, h)) => h.clone(),
            None => {
                let h: Handle<AudioSource> = server.load(path);
                sounds.loaded.push((path, h.clone()));
                h
            }
        }
    };
    world.spawn((Name::new("Titan sound"), AudioPlayer::new(handle), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)).with_speed(speed)));
}
/// Every frame: the titan's breathing when it is near and still (once each breath of its idle).
pub fn update(world: &mut World) {
    let Some((_, frame, root)) = world.resource::<super::Titans>().drawn else {
        return;
    };
    let now = world.resource::<Time<Real>>().elapsed_secs();
    if !world.contains_resource::<Sounds>() {
        world.init_resource::<Sounds>();
    }
    let last = world.resource::<Sounds>().breath;
    // A breath when the chest starts to heave.
    if frame.skeleton.heave > 0.85 && now - last > 2.8 {
        world.resource_mut::<Sounds>().breath = now;
        let mouth = root.transform_point(frame.skeleton.on_head(Vec3::new(0., super::body::GRIN.0, -0.24)));
        play(world, Cue::Breath, mouth, 1.);
    }
}

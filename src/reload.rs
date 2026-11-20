// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Reloading on the client (kitchen-sim magazine.rs): R (controls.rs `Action::Reload`), what this player is shown of
//! its reserve and of the reload under way, the reload in first person and in third, its sounds, and the head-up
//! display's +N when a pickup is walked over.
//!
//! * **State** ([`State`], `Game::mag`). Single player copies the round's own every frame (the reserve, the reload).
//!   Online they come with the server's `{type:'ammo'}` (network.rs → [`hear`]): this player's reserve and reload, and
//!   everyone's (`stocks`), for the third-person cue. R (or the trigger on an empty magazine) starts the reload on this
//!   screen at once ([`foresee`]) and online sends `button::RELOAD` in the next command (online.rs); the server's word
//!   that there is none is taken only GRACE seconds after, as it may not have had the command yet. Taking another gun
//!   in hand cancels it here as it does there.
//! * **First person** ([`hold`] for the gun, [`magazine`] for its magazine, `weapons::Magazine`): the gun is tipped over
//!   towards the free hand, the magazine is pulled and dropped away, a fresh one comes up and is seated with a jolt,
//!   and the action is worked: the mustard pistol's squeeze bottle lifted off its slide and a new one pressed on, the
//!   ketchup sniper's and the hot sauce thrower's bottles and the syrup rifle's jug dropped out below and a new one
//!   pushed up, a fresh glob pushed into the relish bazooka's mouth; then the slide racked, the bolt or the charging
//!   handle pulled, the pump worked. The bug spray and the mayonnaise bottle have no magazine of their own: the empty
//!   one is dropped out of view, a full one comes up, and the can is shaken, the bottle given a squeeze.
//! * **Third person** (player_model.rs `Motion::reload`): the gun comes down a little and the free hand goes to the
//!   magazine, out, down and back in ([`arm`], [`grip`]).
//! * **Sounds** (tools/build_reload_sounds.py): the magazine out, the fresh one seated, the action worked, as the reload
//!   shown passes OUT, IN and RACK.
//! * **The pop** (hud.rs): rounds put into a reserve by walking over a pickup ([`State::gained`]), shown as an
//!   ammunition icon and +N for POP seconds. Not for a hotdog coming back to life, a new round, or rounds that never run
//!   out.
use crate::arena::{Game, Weapon};
use bevy::prelude::*;
use kitchen_sim::feast::GUNS;
use kitchen_sim::magazine::{Reload, Stock, MAGAZINE};

/// Seconds a reload this client foresaw is shown although the server says there is none (it may not have heard).
const GRACE: f32 = 0.6;
/// Seconds the +N pop shows.
pub const POP: f32 = 1.4;
/// Where in a reload (its progress) the magazine comes out, the fresh one is seated, and the action is worked.
pub const OUT: f32 = 0.12;
pub const IN: f32 = 0.62;
pub const RACK: f32 = 0.72;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load).add_systems(Update, follow.after(crate::arena::tick));
    }
}
/// The reload's sounds, by stage: out, in, rack.
#[derive(Resource, Clone)]
pub struct Sounds([Handle<AudioSource>; 3]);
fn load(mut commands: Commands, server: Option<Res<AssetServer>>) {
    let Some(server) = server else { return };
    commands.insert_resource(Sounds(["audio/reload-out.wav", "audio/reload-in.wav", "audio/reload-rack.wav"].map(|path| server.load(path))));
}

/// What this player is shown of its reserves and its reload (see the module's documentation).
#[derive(Clone, Debug, Default)]
pub struct State {
    /// This player's reserve, by gun.
    pub reserve: [i32; GUNS],
    /// The reload under way as it is shown.
    pub reload: Option<Reload>,
    /// Online: every hotdog's reserve and reload as the server last told them.
    pub stocks: Vec<Stock>,
    /// Online: R pressed since the last command (online.rs sends it), and the round's clock when this client last
    /// foresaw a reload.
    pub pending: bool,
    foreseen: f32,
    /// Rounds put into a reserve by a pickup walked over: the gun, how many, and when (`Time<Real>`).
    pub gained: Option<(u8, i32, f32)>,
    /// What the pop is measured from: the round, whether this player was alive, and its reserve, last frame.
    last: Option<(u32, bool, [i32; GUNS])>,
    /// How far the reload shown had got last frame, and of which gun (for the sounds).
    heard: (usize, f32),
}

/// The gun in hand, if the simulation would start reloading it now.
fn can_reload(g: &Game) -> Option<usize> {
    let w = g.weapon as usize;
    ((1..GUNS).contains(&w) && g.mag.reload.is_none() && !g.locked() && g.ammo[w] < MAGAZINE[w] && g.mag.reserve[w] > 0 && !g.feast.rules.unlimited()).then_some(w)
}
/// R: the gun in hand is reloaded. Single player the round does it; online it is shown at once and sent.
pub fn press(g: &mut Game) {
    if !g.network {
        let me = g.me();
        g.feast.reload(me);
        return;
    }
    g.mag.pending = true;
    foresee(g);
}
/// Online: shows the reload of the gun in hand now, if the simulation would start one (R, or the trigger on an empty
/// magazine): the server's word follows.
pub fn foresee(g: &mut Game) {
    if let Some(w) = can_reload(g) {
        let of = g.feast.tune.reload[w];
        g.mag.reload = Some(Reload { gun: w, at: g.elapsed, of, left: of });
        g.mag.foreseen = g.elapsed;
    }
}
/// Online: the server's `{type:'ammo',hotdogs}`.
pub fn hear(g: &mut Game, told: &serde_json::Value) {
    let Ok(stocks) = serde_json::from_value::<Vec<Stock>>(told["hotdogs"].clone()) else {
        return;
    };
    let me = g.me();
    if let Some(mine) = stocks.iter().find(|s| s.id == me) {
        g.mag.reserve = mine.reserve;
        if mine.gun > 0 {
            g.mag.reload = Some(Reload { gun: mine.gun, at: mine.at, of: mine.of, left: (mine.at + mine.of - g.elapsed).clamp(0., mine.of.max(0.)) });
        } else if g.elapsed - g.mag.foreseen > GRACE {
            g.mag.reload = None;
        }
    }
    g.mag.stocks = stocks;
}
/// How far through a reload a hotdog is (0 for none): this player's as shown, anybody else's as the round (single
/// player) or the server (online) has it.
pub fn progress(g: &Game, id: u32) -> f32 {
    let of = |r: Option<Reload>| r.filter(|r| r.left > 0.).map_or(0., |r| r.progress().max(1e-3));
    if id == g.me() {
        return of(g.mag.reload);
    }
    if !g.network {
        return of(g.feast.player(id).and_then(|p| p.reload));
    }
    g.mag.stocks.iter().find(|s| s.id == id && s.gun > 0).map_or(0., |s| if s.of <= 0. { 0. } else { ((g.elapsed - s.at) / s.of).clamp(1e-3, 1.) })
}
/// Every frame, after the round: what is shown is brought up to date and the pop measured. Returns the reload's stages
/// passed since the last frame (0 out, 1 in, 2 rack), for their sounds.
pub fn update(g: &mut Game, now: f32) -> Vec<usize> {
    let me = g.me();
    if !g.network {
        if let Some(p) = g.feast.player(me) {
            (g.mag.reserve, g.mag.reload) = (p.reserve, p.reload);
        }
    } else if let Some(mut r) = g.mag.reload {
        r.left = (r.at + r.of - g.elapsed).min(r.of);
        // (Another gun in hand, or the hands filled: it is over, as it is on the server.)
        g.mag.reload = (r.left > 0. && g.weapon as usize == r.gun).then_some(r);
    }
    // The pop: a reserve that went up while this player stayed alive in the same round, with rounds that run out.
    let alive = g.roster.iter().find(|p| p.id == me).is_some_and(|p| p.alive);
    let reserve = g.mag.reserve;
    if let Some((round, was, before)) = g.mag.last {
        if round == g.feast.round && was && alive && !g.feast.rules.unlimited() {
            let held = g.weapon as usize;
            if let Some((w, n)) = (1..GUNS).map(|w| (w, reserve[w] - before[w])).filter(|(_, n)| *n > 0).max_by_key(|(w, n)| (*w == held, *n)) {
                g.mag.gained = Some((w as u8, n, now));
            }
        }
    }
    g.mag.last = Some((g.feast.round, alive, reserve));
    // The stages the reload shown has passed since the last frame.
    let (gun, u) = g.mag.reload.map_or((0, 0.), |r| (r.gun, r.progress()));
    let (was_gun, was) = g.mag.heard;
    let from = if gun == was_gun { was } else { 0. };
    g.mag.heard = (gun, u);
    [OUT, IN, RACK].into_iter().enumerate().filter(|(_, at)| gun > 0 && from < *at && u >= *at).map(|(k, _)| k).collect()
}
/// The +N pop: the gun and how many, while it shows.
pub fn pop(g: &Game, now: f32) -> Option<(u8, i32, f32)> {
    g.mag.gained.filter(|(.., at)| (0. ..POP).contains(&(now - at)))
}

// ----- How it looks.

/// Smoothstep of `x` from 0 to 1.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}
/// A bump: 0 outside `[a, b]`, up to 1 in the middle.
fn bump(u: f32, a: f32, b: f32) -> f32 {
    if u <= a || u >= b { 0. } else { (std::f32::consts::PI * (u - a) / (b - a)).sin() }
}
/// How far into the reload's pose the gun is held: tipped over through its middle, back as it ends.
fn tilt(u: f32) -> f32 {
    if u <= 0. || u >= 1. { 0. } else { ease(u / OUT) * (1. - ease((u - 0.88) / 0.12)) }
}
/// Whether the gun is reloaded whole (the bug spray, the mayonnaise bottle: no magazine of their own).
pub fn whole(weapon: Weapon) -> bool {
    matches!(weapon, Weapon::Spray | Weapon::Mayo)
}
/// The first-person gun's pose `u` through a reload (0 or none at rest): an offset and a turn, in the view, on top of
/// where it is held. Tipped over towards the free hand and the muzzle up a little, a jolt as the fresh magazine seats,
/// and the action worked: a sharp pull back and forward. A gun reloaded whole drops out of view and comes back up, and
/// is shaken or squeezed.
pub fn hold(weapon: Weapon, u: f32) -> (Vec3, Quat) {
    if !(0. ..1.).contains(&u) || weapon == Weapon::Laden {
        return (Vec3::ZERO, Quat::IDENTITY);
    }
    if whole(weapon) {
        let away = ease((u - 0.08) / 0.22) * (1. - ease((u - 0.42) / 0.22));
        let shake = bump(u, 0.68, 0.92) * (u * 70.).sin();
        let squeeze = if weapon == Weapon::Mayo { bump(u, 0.7, 0.86) } else { 0. };
        return (Vec3::new(0.004 * shake, -0.11 * away - 0.004 * squeeze, 0.02 * away), Quat::from_rotation_x(-0.5 * away) * Quat::from_rotation_z(0.18 * shake));
    }
    let t = tilt(u);
    let seat = bump(u, IN - 0.02, IN + 0.06);
    let rack = bump(u, RACK, RACK + 0.14);
    // (It comes up and in from the bottom corner of the view, so that the magazine going out and the fresh one coming
    // in are seen. The relish bazooka is loaded at its mouth: it is tipped muzzle up to the free hand rather than rolled
    // over.)
    // (Turned so that the magazine faces the eye: the muzzle up for a magazine underneath, down for the pistol's bottle
    // on top of its slide.)
    let (yaw, roll, rise) = match weapon {
        Weapon::Pistol => (0.25, 0.3, -0.55),
        Weapon::Bazooka => (0.3, 0.15, 0.7),
        _ => (0.35, 0.35, 0.8),
    };
    (
        Vec3::new(-0.018 * t, 0.01 * t + 0.003 * seat, 0.012 * t + 0.011 * rack),
        Quat::from_rotation_y(yaw * t) * Quat::from_rotation_z(roll * t) * Quat::from_rotation_x(rise * t - 0.06 * rack + 0.03 * seat),
    )
}
/// Which way a gun's magazine comes out of it, in the gun frame (metres at the end of the pull): the pistol's bottle up
/// off its slide, the bazooka's glob forward out of its mouth (where a fresh one comes from), the rest down.
fn out_of(weapon: Weapon) -> Vec3 {
    match weapon {
        Weapon::Pistol => Vec3::new(0., 0.032, 0.006),
        Weapon::Bazooka => Vec3::new(0., -0.02, -0.05),
        Weapon::Flame => Vec3::new(0., -0.03, -0.012),
        _ => Vec3::new(0., -0.036, -0.003),
    }
}
/// The magazine's place in the gun frame `u` through a reload (`weapons::Magazine`): pulled out (OUT to 0.3) and let
/// drop away out of sight (it falls, and is gone by 0.38), none in the gun while the hand fetches another, the fresh
/// one brought up from further out (from 0.42) and pushed home (by IN). The bazooka's glob was fired: there is none
/// in its mouth until a fresh one is pushed in.
pub fn magazine(weapon: Weapon, u: f32) -> Transform {
    if !(0. ..1.).contains(&u) || whole(weapon) {
        return Transform::IDENTITY;
    }
    let out = out_of(weapon);
    let gone = Transform::from_scale(Vec3::ZERO);
    if u < OUT {
        return Transform::IDENTITY;
    }
    if u < 0.38 {
        if weapon == Weapon::Bazooka {
            return gone;
        }
        let pull = ease((u - OUT) / 0.18);
        let fall = ((u - 0.25) / 0.13).max(0.).powi(2);
        // (The pistol's bottle is tossed off backwards; the rest drop.)
        let spin = if weapon == Weapon::Pistol { Quat::from_rotation_x(1.2 * fall) } else { Quat::from_rotation_z(0.6 * fall) };
        return Transform::from_translation(out * pull + Vec3::new(0.004 * fall, -0.06 * fall, 0.01 * fall)).with_rotation(spin);
    }
    if u < 0.42 {
        return gone;
    }
    let home = ease((u - 0.42) / (IN - 0.42));
    Transform::from_translation(out * 1.6 * (1. - home)).with_rotation(Quat::from_rotation_z(-0.3 * (1. - home)))
}
/// Third person: how much of the reload's arm motion shows `u` through it (0 at rest).
pub fn arm(u: f32) -> f32 {
    if u <= 0. || u >= 1. { 0. } else { ease(u / 0.1) * (1. - ease((u - 0.88) / 0.12)) }
}
/// Third person: where the free hand is in the gun frame `u` through a reload: at the magazine, following it out and
/// down, and back in with the fresh one; then at the action.
pub fn grip(weapon: Weapon, u: f32) -> Vec3 {
    // (`weapons::at` is (ahead, up, side); the gun frame's -z is ahead.)
    let at = |ahead: f32, up: f32| Vec3::new(0., up, -ahead);
    let home = match weapon {
        Weapon::Pistol => at(0.02, 0.022),
        Weapon::Sniper => at(0.022, -0.016),
        Weapon::Flame => at(0.01, -0.016),
        Weapon::Bazooka => at(0.075, 0.014),
        Weapon::Rifle => at(0.024, -0.016),
        Weapon::Spray | Weapon::Mayo | Weapon::Laden => at(0.01, -0.03),
    };
    let away = if whole(weapon) { Vec3::new(0., -0.05, 0.) } else { out_of(weapon) * 1.6 };
    let out = ease((u - OUT) / 0.2) * (1. - ease((u - 0.42) / (IN - 0.42)));
    let action = bump(u, RACK, RACK + 0.14);
    home + away * out + at(-0.02, 0.006) * action
}

/// Each frame of `arena::tick`, after the round has moved: the state brought up to date (`update`) and the reload's
/// sounds played as it passes its stages.
pub fn tick(world: &mut World, g: &mut Game, now: f32) {
    let stages = if g.playing() { update(g, now) } else { vec![] };
    if let Some(sounds) = world.get_resource::<Sounds>().cloned() {
        for k in stages {
            world.spawn((AudioPlayer::new(sounds.0[k].clone()), PlaybackSettings::DESPAWN));
        }
    }
}
/// After `arena::tick` each frame: the first-person gun and its magazine posed on top of where it holds them.
pub(crate) fn follow(world: &mut World) {
    let Some(g) = world.remove_resource::<Game>() else { return };
    let u = g.mag.reload.filter(|r| r.gun == g.weapon as usize && r.left > 0.).map_or(-1., |r| r.progress());
    for &(weapon, root) in g.viewmodels() {
        let shown = if weapon == g.weapon { u } else { -1. };
        let (at, turn) = hold(weapon, shown);
        if let Some(mut tr) = world.get_mut::<Transform>(root) {
            tr.translation += at;
            tr.rotation *= turn;
        }
        // (The magazine hangs from the gun's own root, under the first-person root with the hands.)
        let mut below = world.get::<Children>(root).map(|c| c.to_vec()).unwrap_or_default();
        while let Some(child) = below.pop() {
            if world.get::<crate::weapons::Magazine>(child).is_some() {
                let place = magazine(weapon, shown);
                if let Some(mut tr) = world.get_mut::<Transform>(child) {
                    if *tr != place {
                        *tr = place;
                    }
                }
            } else if let Some(children) = world.get::<Children>(child) {
                below.extend(children.iter());
            }
        }
    }
    world.insert_resource(g);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{Context, Rounds, Show};

    fn frames(app: &mut App, seconds: f32) {
        for _ in 0..(seconds * 60.).round() as usize {
            crate::arena::test_frame(app, 1. / 60.);
        }
    }
    fn press(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        crate::arena::test_frame(app, 1. / 60.);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
    }
    fn rounds(app: &App) -> (Option<Rounds>, Option<u8>) {
        let g = app.world().resource::<Game>();
        let arms = Show::of(g, &Context::default()).arms.unwrap();
        (arms.rounds, arms.reload)
    }

    /// Single player: R reloads the gun in hand (the simulation does it), the display shows the magazine and the
    /// reserve beside the bullet, a bar that fills while the reload runs, and the magazine full after it.
    #[test]
    fn r_reloads_the_gun_in_hand_and_the_display_shows_the_magazine_the_reserve_and_the_reload_filling() {
        use kitchen_sim::feast::RIFLE;
        use kitchen_sim::magazine::RELOAD;
        let mut app = crate::arena::test_app("");
        app.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        let p = &mut app.world_mut().resource_mut::<Game>().feast.players[0];
        (p.ammo[RIFLE], p.reserve[RIFLE]) = (20, 100);
        crate::arena::test_frame(&mut app, 0.05);
        assert_eq!(rounds(&app), (Some(Rounds::Count { now: 20, full: 60, reserve: 100 }), None));
        press(&mut app, KeyCode::KeyR);
        assert!(app.world().resource::<Game>().feast.reloading(1), "R reloads");
        frames(&mut app, RELOAD[RIFLE] * 0.5);
        let (_, bar) = rounds(&app);
        assert!(bar.is_some_and(|eighths| (3..=5).contains(&eighths)), "half way: {bar:?}");
        frames(&mut app, RELOAD[RIFLE] * 0.5 + 0.1);
        assert_eq!(rounds(&app), (Some(Rounds::Count { now: 60, full: 60, reserve: 60 }), None));
        // Another gun in hand cancels it: nothing goes in.
        app.world_mut().resource_mut::<Game>().feast.players[0].ammo[RIFLE] = 30;
        crate::arena::test_frame(&mut app, 0.05);
        press(&mut app, KeyCode::KeyR);
        press(&mut app, KeyCode::Digit2);
        frames(&mut app, RELOAD[RIFLE] + 0.1);
        let p = &app.world().resource::<Game>().feast.players[0];
        assert_eq!((p.ammo[RIFLE], p.reserve[RIFLE], p.weapon), (30, 60, 1));
    }

    /// A pickup walked over that puts rounds into a reserve pops up as an ammunition icon and +N for a moment.
    #[test]
    fn rounds_walked_over_pop_up_as_plus_n_and_a_respawn_does_not() {
        let mut app = crate::arena::test_app("");
        app.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        crate::arena::test_frame(&mut app, 0.05);
        app.world_mut().resource_mut::<Game>().feast.players[0].reserve[1] += 12;
        crate::arena::test_frame(&mut app, 0.05);
        let now = app.world().resource::<Time<Real>>().elapsed_secs();
        assert_eq!(pop(app.world().resource::<Game>(), now).map(|(w, n, _)| (w, n)), Some((1, 12)));
        frames(&mut app, POP + 0.1);
        let now = app.world().resource::<Time<Real>>().elapsed_secs();
        assert_eq!(pop(app.world().resource::<Game>(), now), None, "for a moment");
        // Dead and back with a fresh loadout: no pop.
        let p = &mut app.world_mut().resource_mut::<Game>().feast.players[0];
        (p.alive, p.hp) = (false, 0.);
        crate::arena::test_frame(&mut app, 0.05);
        let p = &mut app.world_mut().resource_mut::<Game>().feast.players[0];
        (p.alive, p.hp, p.reserve[1]) = (true, 100., 40);
        crate::arena::test_frame(&mut app, 0.05);
        let now = app.world().resource::<Time<Real>>().elapsed_secs();
        assert_eq!(pop(app.world().resource::<Game>(), now), None);
    }

    /// Third person: a hotdog reloading brings its free hand to the gun, away from where it hangs otherwise.
    #[test]
    fn in_third_person_the_free_hand_works_the_magazine() {
        use crate::player_model::{pose, Motion};
        let at_rest = pose(Motion { weapon: 2, aim: 1., ..default() });
        let reloading = pose(Motion { weapon: 2, aim: 1., reload: 0.3, ..default() });
        assert!(reloading.shoulders[0].angle_between(at_rest.shoulders[0]) > 0.3, "the free arm comes across");
        assert!(reloading.shoulders[1].angle_between(at_rest.shoulders[1]) > 0.05, "the gun comes down a little");
        let done = pose(Motion { weapon: 2, aim: 1., reload: 0.999, ..default() });
        assert!(done.shoulders[0].angle_between(at_rest.shoulders[0]) < 0.05);
    }

    #[test]
    fn the_gun_is_at_rest_outside_a_reload_and_moves_through_one_and_back() {
        for weapon in [Weapon::Pistol, Weapon::Sniper, Weapon::Flame, Weapon::Bazooka, Weapon::Spray, Weapon::Rifle, Weapon::Mayo] {
            for u in [-1., 0., 1.] {
                assert_eq!(hold(weapon, u), (Vec3::ZERO, Quat::IDENTITY), "{weapon:?} at {u}");
                assert_eq!(magazine(weapon, u), Transform::IDENTITY, "{weapon:?} at {u}");
                assert_eq!(arm(u), 0.);
            }
            let (at, turn) = hold(weapon, 0.5);
            assert!(at.length() > 0.005 || turn.angle_between(Quat::IDENTITY) > 0.1, "{weapon:?} moves");
            // Nearly back as it ends: no jump when the reload is over.
            let (at, turn) = hold(weapon, 0.995);
            assert!(at.length() < 0.002 && turn.angle_between(Quat::IDENTITY) < 0.02, "{weapon:?}: {at} {turn}");
        }
    }

    #[test]
    fn the_magazine_comes_out_drops_out_of_sight_and_a_fresh_one_is_pushed_home() {
        for weapon in [Weapon::Pistol, Weapon::Sniper, Weapon::Flame, Weapon::Rifle] {
            let out = magazine(weapon, 0.3);
            assert!(out.translation.length() > 0.02 && out.scale == Vec3::ONE, "{weapon:?} pulled out: {out:?}");
            assert_eq!(magazine(weapon, 0.4).scale, Vec3::ZERO, "{weapon:?}: dropped, and gone");
            let coming = magazine(weapon, 0.5);
            assert!(coming.scale == Vec3::ONE && coming.translation.length() > 0.01, "{weapon:?} a fresh one on its way");
            let home = magazine(weapon, IN + 0.01);
            assert!(home.translation.length() < 1e-3, "{weapon:?} seated: {home:?}");
        }
        // The pistol's bottle comes up off its slide; the sniper's and the rifle's drop out below.
        assert!(magazine(Weapon::Pistol, 0.28).translation.y > 0.01);
        assert!(magazine(Weapon::Sniper, 0.28).translation.y < -0.01 && magazine(Weapon::Rifle, 0.33).translation.y < -0.03);
        // The bazooka's glob was fired: none until a fresh one goes in at its mouth.
        assert_eq!(magazine(Weapon::Bazooka, 0.2).scale, Vec3::ZERO);
        assert!(magazine(Weapon::Bazooka, 0.5).translation.z < -0.01, "from in front");
        // The can and the bottle go whole.
        assert!(hold(Weapon::Spray, 0.3).0.y < -0.05 && hold(Weapon::Mayo, 0.3).0.y < -0.05);
    }

    #[test]
    fn the_free_hand_goes_to_the_magazine_follows_it_out_and_back_and_the_stages_are_in_order() {
        assert!(0. < OUT && OUT < IN && IN < RACK && RACK < 1.);
        for weapon in [Weapon::Pistol, Weapon::Sniper, Weapon::Rifle] {
            let (at, away, back) = (grip(weapon, 0.05), grip(weapon, 0.32), grip(weapon, IN + 0.02));
            assert!(away.distance(at) > 0.02 && back.distance(at) < 0.005, "{weapon:?}: {at} {away} {back}");
        }
        assert!(arm(0.5) > 0.99 && arm(0.02) < 0.2);
    }
}

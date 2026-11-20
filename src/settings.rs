// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The player's settings that the menu changes (menu.rs), each kept for next time (prefs.rs) and applied at once:
//!
//! * **Mouse sensitivity**, a factor on the look's own speed (1 is the game's, `arena::tick`), and **invert Y**. They
//!   apply to the mouse in first person, at the card table and round the death camera; a touch drag keeps the speed
//!   set on the page (touch.js).
//! * **Field of view**: the first person's vertical angle, in degrees (85 to begin with). The ketchup sniper's scope
//!   stays 18 degrees, and the syrup rifle's sights zoom from whatever it is.
//! * **Volume**: every sound the game plays, through Bevy's `GlobalVolume` (a sound already playing keeps the volume it
//!   started at; the condiments' streams follow at once, squirt.rs). The game has sound effects and no music.
//! * **Damage indicators** (damage.rs): the arcs round the crosshair that point where a hit came from, and the flash at
//!   the edge of the screen nearest it. On to begin with. (The hit marker at the crosshair is always shown.)
//! * **Kill slow motion** (slowmo.rs): a moment of slow motion when this player's shot kills. On to begin with.
//! * **Scope**: hold or toggle. Held (to begin with), the right button keeps the ketchup sniper's scope or the syrup
//!   rifle's optic up while it is down; toggled, one press puts it up and the next takes it down (`arena::tick`; the
//!   touch controls' Scope button always toggles).
//!
//! The hints (hints.rs) and the graphics (quality.rs) are kept where they always were.
use bevy::prelude::*;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::prefs::Prefs>().init_resource::<Settings>().add_systems(Startup, load).add_systems(Update, (apply, remember));
    }
}

/// The ranges, and what each starts at.
pub const SENSITIVITY: (f32, f32, f32) = (0.2, 3., 1.);
pub const FOV: (f32, f32, f32) = (65., 105., 85.);
pub const VOLUME: (f32, f32, f32) = (0., 100., 100.);

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// A factor on the mouse's look.
    pub sensitivity: f32,
    pub invert_y: bool,
    /// Degrees, up and down the screen.
    pub fov: f32,
    /// Per cent.
    pub volume: f32,
    /// The arcs round the crosshair and the flash at the screen's edge that show where a hit came from (damage.rs).
    pub damage_indicators: bool,
    /// Slow motion when this player's shot kills (slowmo.rs).
    pub kill_slowmo: bool,
    /// The right button toggles the scope instead of holding it up (`arena::tick`).
    pub scope_toggle: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self { sensitivity: SENSITIVITY.2, invert_y: false, fov: FOV.2, volume: VOLUME.2, damage_indicators: true, kill_slowmo: true, scope_toggle: false }
    }
}
impl Settings {
    /// What the mouse's motion is multiplied by, each way, before it turns the view.
    pub fn look(&self) -> Vec2 {
        Vec2::new(self.sensitivity, if self.invert_y { -self.sensitivity } else { self.sensitivity })
    }
    /// The first person's field of view, in radians.
    pub fn fov_radians(&self) -> f32 {
        self.fov.to_radians()
    }
    /// The global volume's factor: the slider's share squared, which sounds even all along it.
    pub fn gain(&self) -> f32 {
        (self.volume / 100.).clamp(0., 1.).powi(2)
    }
    /// Each value within its range (a kept value from elsewhere may not be).
    pub fn clamped(self) -> Self {
        let within = |v: f32, (lo, hi, start): (f32, f32, f32)| if v.is_finite() { v.clamp(lo, hi) } else { start };
        Self { sensitivity: within(self.sensitivity, SENSITIVITY), fov: within(self.fov, FOV), volume: within(self.volume, VOLUME), ..self }
    }
    /// As kept: one key each.
    pub fn read(prefs: &crate::prefs::Prefs) -> Self {
        let number = |key: &str, start: f32| prefs.get(key).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(start);
        // (A switch that is on to begin with is on unless it was kept off.)
        let switch = |key: &str, start: bool| prefs.get(key).map_or(start, |v| v.trim() == "on");
        Settings {
            sensitivity: number(KEYS[0], SENSITIVITY.2),
            invert_y: switch(KEYS[1], false),
            fov: number(KEYS[2], FOV.2),
            volume: number(KEYS[3], VOLUME.2),
            damage_indicators: switch(KEYS[4], true),
            kill_slowmo: switch(KEYS[5], true),
            scope_toggle: switch(KEYS[6], false),
        }
        .clamped()
    }
    fn write(&self, was: Option<&Settings>, prefs: &mut crate::prefs::Prefs) {
        let changed = |f: fn(&Settings) -> String| was.is_none_or(|w| f(w) != f(self));
        fn on(b: bool) -> String {
            if b { "on".into() } else { "off".into() }
        }
        let values: [fn(&Settings) -> String; 7] = [
            |s| format!("{:.2}", s.sensitivity),
            |s| on(s.invert_y),
            |s| format!("{:.0}", s.fov),
            |s| format!("{:.0}", s.volume),
            |s| on(s.damage_indicators),
            |s| on(s.kill_slowmo),
            |s| on(s.scope_toggle),
        ];
        for (key, value) in KEYS.iter().zip(values) {
            if changed(value) {
                prefs.set(key, &value(self));
            }
        }
    }
}
/// Where each is kept (prefs.rs).
pub const KEYS: [&str; 7] = ["look.sensitivity", "look.invert", "fov", "volume", "damage.indicators", "kill.slowmo", "scope.toggle"];

/// Startup: the settings kept from last time.
pub fn load(mut settings: ResMut<Settings>, prefs: Res<crate::prefs::Prefs>) {
    *settings = Settings::read(&prefs);
}
/// The volume, whenever it changes.
fn apply(settings: Res<Settings>, volume: Option<ResMut<GlobalVolume>>) {
    if let Some(mut volume) = volume.filter(|_| settings.is_changed()) {
        volume.volume = bevy::audio::Volume::Linear(settings.gain());
    }
}
/// Seconds a setting is left alone before it is written: a slider being dragged is written once it stops.
const SETTLE: f32 = 0.4;
/// Every frame: settings that changed are kept for next time (only the ones that changed are written), once they have
/// stopped changing for a moment.
fn remember(settings: Res<Settings>, time: Option<Res<Time<Real>>>, mut prefs: ResMut<crate::prefs::Prefs>, mut kept: Local<Option<Settings>>, mut since: Local<Option<(Settings, f32)>>) {
    // (Without a clock, as in a test, a change is written on the next frame.)
    let now = time.map_or(f32::INFINITY, |t| t.elapsed_secs());
    let Some(was) = *kept else {
        *kept = Some(*settings);
        return;
    };
    if was == *settings {
        *since = None;
        return;
    }
    match *since {
        Some((last, at)) if last == *settings && (now - at >= SETTLE || now.is_infinite()) => {
            settings.write(Some(&was), &mut prefs);
            (*kept, *since) = (Some(*settings), None);
        }
        Some((last, _)) if last == *settings => {}
        _ => *since = Some((*settings, now)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_kept_read_back_and_kept_within_range() {
        let mut prefs = crate::prefs::Prefs::memory();
        assert_eq!(Settings::read(&prefs), Settings::default());
        let mine = Settings { sensitivity: 1.75, invert_y: true, fov: 95., volume: 40., damage_indicators: false, kill_slowmo: false, scope_toggle: true };
        mine.write(None, &mut prefs);
        assert_eq!(Settings::read(&prefs), mine);
        prefs.set("fov", "400");
        prefs.set("look.sensitivity", "nonsense");
        let read = Settings::read(&prefs);
        assert_eq!((read.fov, read.sensitivity), (FOV.1, SENSITIVITY.2));
        // Invert Y turns the vertical look the other way; the sensitivity scales both.
        assert_eq!(mine.look(), Vec2::new(1.75, -1.75));
        assert_eq!(Settings::default().look(), Vec2::ONE);
        assert!((Settings { volume: 50., ..default() }.gain() - 0.25).abs() < 1e-6 && Settings::default().gain() == 1.);
    }

    /// A change made in the game is kept for the next, once it stops changing (a slider being dragged is not written at
    /// every step of the drag).
    #[test]
    fn a_change_is_kept_once_it_settles() {
        let mut app = App::new();
        app.insert_resource(crate::prefs::Prefs::memory()).init_resource::<Time<Real>>().init_resource::<Settings>().add_systems(Startup, load).add_systems(Update, remember);
        let frame = |app: &mut App, dt: f32| {
            app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_secs_f32(dt));
            app.update();
        };
        frame(&mut app, 0.016);
        for step in 0..10 {
            app.world_mut().resource_mut::<Settings>().fov = 90. + step as f32;
            frame(&mut app, 0.016);
            assert_eq!(app.world().resource::<crate::prefs::Prefs>().get("fov"), None, "not while it is being dragged");
        }
        frame(&mut app, 0.25);
        frame(&mut app, 0.25);
        assert_eq!(app.world().resource::<crate::prefs::Prefs>().get("fov").as_deref(), Some("99"));
        assert_eq!(app.world().resource::<crate::prefs::Prefs>().get("volume"), None, "only what changed is written");
    }

    /// The scope is held up by the right button to begin with; the menu's chooser makes it toggle, and what it was left
    /// at is kept for next time.
    #[test]
    fn the_scope_is_held_to_begin_with_and_toggling_it_in_the_menu_is_kept() {
        assert!(!Settings::default().scope_toggle);
        let mut app = App::new();
        app.insert_resource(crate::prefs::Prefs::memory()).init_resource::<Time<Real>>().init_resource::<Settings>().add_systems(Startup, load).add_systems(Update, remember);
        let frame = |app: &mut App, dt: f32| {
            app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_secs_f32(dt));
            app.update();
        };
        frame(&mut app, 0.016);
        crate::menu::scope(app.world_mut());
        assert!(app.world().resource::<Settings>().scope_toggle);
        for _ in 0..3 {
            frame(&mut app, 0.25);
        }
        let prefs = app.world().resource::<crate::prefs::Prefs>();
        assert_eq!(prefs.get("scope.toggle").as_deref(), Some("on"));
        assert!(Settings::read(prefs).scope_toggle, "toggled next time");
        crate::menu::scope(app.world_mut());
        for _ in 0..3 {
            frame(&mut app, 0.25);
        }
        let prefs = app.world().resource::<crate::prefs::Prefs>();
        assert_eq!(prefs.get("scope.toggle").as_deref(), Some("off"));
        assert!(!Settings::read(prefs).scope_toggle, "held again");
    }

    /// The damage indicators and the kill slow motion are on to begin with; the menu's switches turn them off, and what
    /// they were left at is kept for next time.
    #[test]
    fn the_damage_indicator_and_kill_slow_motion_switches_are_on_to_begin_with_and_kept() {
        assert!(Settings::default().damage_indicators && Settings::default().kill_slowmo);
        let mut app = App::new();
        app.insert_resource(crate::prefs::Prefs::memory())
            .init_resource::<Time<Real>>()
            .init_resource::<Settings>()
            .init_resource::<crate::controls::Bindings>()
            .add_systems(Startup, load)
            .add_systems(Update, remember);
        let frame = |app: &mut App, dt: f32| {
            app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_secs_f32(dt));
            app.update();
        };
        frame(&mut app, 0.016);
        crate::menu::toggle(app.world_mut(), crate::menu::Toggle::DamageIndicators);
        crate::menu::toggle(app.world_mut(), crate::menu::Toggle::SlowMotion);
        let s = *app.world().resource::<Settings>();
        assert!(!s.damage_indicators && !s.kill_slowmo);
        for _ in 0..3 {
            frame(&mut app, 0.25);
        }
        let prefs = app.world().resource::<crate::prefs::Prefs>();
        assert_eq!((prefs.get("damage.indicators").as_deref(), prefs.get("kill.slowmo").as_deref()), (Some("off"), Some("off")));
        let read = Settings::read(prefs);
        assert!(!read.damage_indicators && !read.kill_slowmo, "off next time");
        // And back on.
        crate::menu::toggle(app.world_mut(), crate::menu::Toggle::SlowMotion);
        for _ in 0..3 {
            frame(&mut app, 0.25);
        }
        let read = Settings::read(app.world().resource::<crate::prefs::Prefs>());
        assert!(!read.damage_indicators && read.kill_slowmo);
    }

    /// Invert Y turns the vertical look over wherever the mouse looks: in first person, through the sniper's scope, and
    /// over the shoulder in Kung Fu (whose camera follows the same aim). The death camera's climb is deathcam.rs's test;
    /// the card table reads the same `look`. (A touch drag keeps the page's own way, touch.js: the switch is the mouse's.)
    #[test]
    fn invert_y_turns_the_look_over_in_first_person_through_the_scope_and_over_the_shoulder() {
        use crate::arena::{test_app, test_frame, Game, Weapon};
        use bevy::input::mouse::AccumulatedMouseMotion;
        use bevy::window::{CursorGrabMode, CursorOptions};
        // How the aim's pitch and the camera's tilt change for the mouse moved down.
        let moved = |invert: bool, scoped: bool, fu: bool| -> (f32, f32) {
            let mut app = test_app("");
            app.insert_resource(Settings { invert_y: invert, ..default() });
            if fu {
                app.add_plugins(crate::fight::Plugin);
            }
            app.add_systems(Update, crate::arena::tick);
            if fu {
                let map = kitchen_sim::map::Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/kung-fu-dojo.json")).unwrap()).unwrap();
                let mut g = app.world_mut().resource_mut::<Game>();
                g.feast = kitchen_sim::feast::Feast::with_map(map, false);
                g.begin();
            }
            let window = app.world_mut().query_filtered::<Entity, With<Window>>().iter(app.world()).next().unwrap();
            app.world_mut().entity_mut(window).insert(CursorOptions { grab_mode: CursorGrabMode::Locked, ..default() });
            for _ in 0..3 {
                test_frame(&mut app, 0.05);
            }
            if fu {
                assert!(app.world().resource::<Game>().kung_fu());
                app.world_mut().resource_mut::<crate::fight::Camera>().third = true;
                test_frame(&mut app, 0.05);
            }
            if scoped {
                let mut g = app.world_mut().resource_mut::<Game>();
                if let Some(p) = g.feast.player_mut(1) {
                    (p.owned[Weapon::Sniper as usize], p.ammo[Weapon::Sniper as usize]) = (true, 6);
                }
                g.owned[Weapon::Sniper as usize] = true;
                (g.weapon, g.scoped) = (Weapon::Sniper, true);
            }
            let camera = app.world_mut().query_filtered::<Entity, With<crate::maya::HotdogCamera>>().single(app.world()).unwrap();
            let tilt = |app: &App| app.world().get::<Transform>(camera).unwrap().forward().y;
            let (pitch, look) = (app.world().resource::<Game>().pitch, tilt(&app));
            app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(0., 30.);
            test_frame(&mut app, 0.05);
            if scoped {
                assert!(app.world().resource::<Game>().scoped, "still looking through the scope");
            }
            (app.world().resource::<Game>().pitch - pitch, tilt(&app) - look)
        };
        for (scoped, fu) in [(false, false), (true, false), (false, true)] {
            let (down, up) = (moved(false, scoped, fu), moved(true, scoped, fu));
            assert!(down.0 < 0. && down.1 < 0., "scoped {scoped}, kung fu {fu}: the mouse moved down looks down: {down:?}");
            assert!(up.0 > 0. && up.1 > 0., "scoped {scoped}, kung fu {fu}: turned over, it looks up: {up:?}");
            assert!((down.0 + up.0).abs() < 1e-5, "as far either way");
        }
    }

}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Kill slow motion: when this player's shot (or blow, or the last blow of a Kung Fu KO) kills another hotdog, time
//! slows to a quarter for a moment and comes back. damage.rs, which sees the kills, starts it ([`SlowMo::kill`]).
//!
//! * **Single player** slows for real. `arena::tick` steps the round, this player's walk and the round's own clocks by
//!   the slowed frame ([`sim_scale`]), the hotdogs are animated on the slowed clock ([`clock`]) so their legs keep step
//!   with how far they go, and the ragdolls fall at the same pace (death.rs). It is SLOW (0.25) for HOLD (0.6) seconds
//!   of real time, easing in over EASE_IN and back over EASE_OUT. Another kill while it lasts holds it longer, never to
//!   more than CAP (1.2) seconds in all; once it is over a new one may not start for REST seconds, so a spree is not one
//!   long slow motion.
//! * **Online** the round's clock is the server's and nothing here changes it, nor anything sent: the slow motion is for
//!   this screen alone ([`SlowMo::shown_only`]). The victim's fall (its ragdoll, death.rs) plays slowed, with the same
//!   push of the camera and pulse of colour and the sounds pitched down a little, while the player's own controls, its
//!   prediction and everything drawn from the server's timeline (the other hotdogs, the cats, the rounds in the air and
//!   their splashes: arena/online.rs) go on at their own pace underneath. The killing round's last stretch is not slowed
//!   online: it is drawn from the timeline, a little in the past, and slowing it alone would put it behind the fall it
//!   caused.
//! * **The round's last kill**, for everybody: a death that ends the round has its fall shown slowed, as a short
//!   flourish (for the screen alone: the round is over).
//! * **What shows**, by how deep the slow motion is ([`SlowMo::level`]): the camera pushes in a little towards the victim
//!   (a narrower view and a step nearer, never turned: the aim stays where the crosshair is), the picture loses colour
//!   and gains contrast (the game camera's colour grade, visuals.rs, which viewmodel.rs hands on), the edges darken, and
//!   the sounds playing are pitched down.
//!
//! Off in the settings (settings.rs `kill_slowmo`), none of it happens, and one under way stops.
use crate::arena::Game;
use bevy::audio::AudioSinkPlayback;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use bevy::ui::{BackgroundGradient, ColorStop, RadialGradient, RadialGradientShape, UiPosition};

/// The pace at the bottom of the slow motion.
pub const SLOW: f32 = 0.25;
/// Seconds of real time a kill holds it slow (easing in included), the most a chain of kills holds it for, the seconds
/// it takes to slow and to come back, and the seconds after one before another may begin.
pub const HOLD: f32 = 0.6;
pub const CAP: f32 = 1.2;
pub const EASE_IN: f32 = 0.06;
pub const EASE_OUT: f32 = 0.4;
pub const REST: f32 = 1.;
/// At the deepest: the share the view narrows by, how far (metres) the camera steps towards the victim, the share of the
/// colour taken out and the contrast added, how dark the edges go, and how far the sounds are pitched down (for the
/// screen alone, less: the round under it is going at its pace).
const ZOOM: f32 = 0.12;
const PUSH: f32 = 0.012;
const DESATURATE: f32 = 0.6;
const CONTRAST: f32 = 0.22;
const SHADE: f32 = 0.62;
const PITCH: [f32; 2] = [0.35, 0.18];

#[derive(Resource, Clone, Debug, Default)]
pub struct SlowMo {
    /// The real time (`Time<Real>`, seconds) it began, and the seconds it is held slow for.
    pub start: Option<f32>,
    pub hold: f32,
    /// For this screen alone (online, or a round already over): the round goes on at its own pace.
    pub shown_only: bool,
    /// Where the victim fell: the camera pushes in that way.
    pub focus: Option<Vec3>,
    /// Seconds of real time the slowed clock has fallen behind ([`clock`]).
    pub lost: f32,
    /// A replay's speed (replay/: 0.25 to 2, 0 paused), which the round and the falls go at; None otherwise.
    pub replay: Option<f32>,
    /// What was done to the camera, the grade and the sounds, to be undone.
    lens: Option<Lens>,
    grade: Option<([f32; 4], [f32; 4])>,
    pitched: bool,
    shade: Option<Entity>,
}
/// The camera's place and view as the frame had them, and as they were written here.
#[derive(Clone, Copy, Debug)]
struct Lens {
    base: (Transform, f32),
    written: (Transform, f32),
}

impl SlowMo {
    /// A kill at `now`: slow motion begins, or one under way is held longer (up to CAP in all). `shown_only` is for the
    /// screen alone; `focus` where the victim fell. Whether it is on now.
    pub fn kill(&mut self, now: f32, shown_only: bool, focus: Option<Vec3>) -> bool {
        if let Some(start) = self.start {
            let t = now - start;
            if (0. ..self.hold + EASE_OUT).contains(&t) {
                self.hold = self.hold.max((t + HOLD).min(CAP));
                self.shown_only &= shown_only;
                self.focus = focus.or(self.focus);
                return true;
            }
            if t >= 0. && t < self.hold + EASE_OUT + REST {
                return false;
            }
        }
        let rest = std::mem::take(self);
        *self = SlowMo { start: Some(now), hold: HOLD, shown_only, focus, ..rest };
        true
    }
    /// The pace at `now`: 1 (none) to SLOW.
    pub fn scale(&self, now: f32) -> f32 {
        let Some(start) = self.start else { return 1. };
        let t = now - start;
        if !(0. ..self.hold + EASE_OUT).contains(&t) {
            return 1.;
        }
        if t < EASE_IN {
            1. + (SLOW - 1.) * smooth(t / EASE_IN)
        } else if t < self.hold {
            SLOW
        } else {
            SLOW + (1. - SLOW) * smooth((t - self.hold) / EASE_OUT)
        }
    }
    /// How deep it is at `now`: 0 (none) to 1 (SLOW).
    pub fn level(&self, now: f32) -> f32 {
        ((1. - self.scale(now)) / (1. - SLOW)).clamp(0., 1.)
    }
    /// The pace the round itself goes at: the same, unless it is for the screen alone.
    pub fn sim_scale(&self, now: f32) -> f32 {
        if self.shown_only { 1. } else { self.scale(now) }
    }
    /// Whether it is under way at `now`.
    pub fn running(&self, now: f32) -> bool {
        self.scale(now) < 1.
    }
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// The pace `arena::tick` steps the round at this frame: single player's slow motion, and always 1 online (the round's
/// clock is the server's) or without a game.
pub fn sim_scale(world: &World) -> f32 {
    let Some(s) = world.get_resource::<SlowMo>() else { return 1. };
    if let Some(pace) = s.replay {
        return pace;
    }
    if world.get_resource::<Game>().is_none_or(|g| g.network) {
        return 1.;
    }
    s.sim_scale(world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs()))
}
/// The slowed clock at real time `now`: real time, less what slow motion has held back (the hotdogs are animated on it
/// offline, so that their legs and swings slow with their steps).
pub fn clock(world: &World, now: f32) -> f32 {
    world.get_resource::<SlowMo>().map_or(now, |s| now - s.lost)
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SlowMo>().add_systems(
            Update,
            present.after(crate::arena::tick).after(crate::deathcam::watch).after(crate::kungfu::view::update),
        );
    }
}

/// Every frame, after everything has placed the camera: the slowed clock moves on, and the slow motion shows (or what
/// it did is undone).
pub fn present(world: &mut World) {
    let dt = world.get_resource::<Time<Real>>().map_or(0., |t| t.delta_secs());
    // (A replay watched through a player's recorded view goes by that player's clock: replay/pov.rs.)
    let now = crate::replay::pov::now(world);
    let on = world.get_resource::<crate::settings::Settings>().is_none_or(|s| s.kill_slowmo);
    let Some(mut s) = world.remove_resource::<SlowMo>() else { return };
    if !on && s.running(now) {
        s.start = None;
    }
    let scale = s.scale(now);
    if !s.shown_only {
        s.lost += dt.min(0.25) * (1. - scale);
    }
    let level = s.level(now);
    lens(world, &mut s, level);
    grade(world, &mut s, level);
    pitch(world, &mut s, level);
    shade(world, &mut s, level);
    world.insert_resource(s);
}

/// The push in: a narrower view and a step towards the victim, in first person (or over the shoulder) while alive.
fn lens(world: &mut World, s: &mut SlowMo, level: f32) {
    let (cam, first) = match world.get_resource::<Game>() {
        Some(g) => (g.camera(), g.playing() && !g.demo && !crate::cards::at_table(g) && g.roster.iter().any(|p| p.id == g.me() && p.alive)),
        None => (None, false),
    };
    let Some(cam) = cam else {
        s.lens = None;
        return;
    };
    let (Some(tr), Some(Projection::Perspective(p))) = (world.get::<Transform>(cam).copied(), world.get::<Projection>(cam).cloned()) else {
        s.lens = None;
        return;
    };
    // What the frame put there; or, when nothing has put the camera anywhere since (single player paused), what was
    // there before this pushed it.
    let base = match s.lens {
        Some(l) if l.written.0 == tr && l.written.1 == p.fov => l.base,
        _ => (tr, p.fov),
    };
    let write = |world: &mut World, (tr, fov): (Transform, f32)| {
        if let Some(mut t) = world.get_mut::<Transform>(cam) {
            *t = tr;
        }
        if let Some(mut projection) = world.get_mut::<Projection>(cam) {
            if let Projection::Perspective(p) = projection.as_mut() {
                p.fov = fov;
            }
        }
    };
    if level <= 0. || !first {
        if s.lens.is_some() && base != (tr, p.fov) {
            write(world, base);
        }
        s.lens = None;
        return;
    }
    let ahead = *base.0.forward();
    let toward = s.focus.map_or(ahead, |f| (f - base.0.translation).normalize_or(ahead));
    let pushed = (Transform { translation: base.0.translation + toward * PUSH * level, ..base.0 }, base.1 * (1. - ZOOM * level));
    write(world, pushed);
    s.lens = Some(Lens { base, written: pushed });
}

/// The colour pulse: the game camera's grade loses colour and gains contrast (viewmodel.rs hands the grade on to the
/// camera that finishes the picture). Without a grade (the minimal graphics) there is none.
fn grade(world: &mut World, s: &mut SlowMo, level: f32) {
    let cam = world.get_resource::<Game>().and_then(|g| g.camera());
    let Some(mut grading) = cam.and_then(|cam| world.get_mut::<ColorGrading>(cam)) else {
        s.grade = None;
        return;
    };
    let read = |g: &ColorGrading| [g.global.post_saturation, g.shadows.contrast, g.midtones.contrast, g.highlights.contrast];
    let set = |g: &mut ColorGrading, v: [f32; 4]| {
        (g.global.post_saturation, g.shadows.contrast, g.midtones.contrast, g.highlights.contrast) = (v[0], v[1], v[2], v[3]);
    };
    let now = read(&grading);
    let base = match s.grade {
        Some((base, written)) if written == now => base,
        _ => now,
    };
    if level <= 0. {
        if s.grade.is_some() && base != now {
            set(&mut grading, base);
        }
        s.grade = None;
        return;
    }
    let pulsed = [base[0] * (1. - DESATURATE * level), base[1] * (1. + CONTRAST * level), base[2] * (1. + CONTRAST * level), base[3] * (1. + CONTRAST * level)];
    set(&mut grading, pulsed);
    s.grade = Some((base, pulsed));
}

/// The sounds playing pitched down (each at its own speed times the factor), and back once it is over.
fn pitch(world: &mut World, s: &mut SlowMo, level: f32) {
    if level <= 0. && !s.pitched {
        return;
    }
    let factor = 1. - PITCH[s.shown_only as usize] * level;
    for (sink, playing) in world.query::<(&AudioSink, Option<&PlaybackSettings>)>().iter(world) {
        let want = playing.map_or(1., |p| p.speed) * factor;
        if (sink.speed() - want).abs() > 1e-3 {
            sink.set_speed(want);
        }
    }
    s.pitched = level > 0.;
}

/// The edges darken (under the head-up display).
fn shade(world: &mut World, s: &mut SlowMo, level: f32) {
    let alive = s.shade.filter(|e| world.get_entity(*e).is_ok());
    if level <= 0. {
        if let Some(e) = alive {
            if world.get::<Visibility>(e).is_some_and(|v| *v != Visibility::Hidden) {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
        }
        return;
    }
    let e = alive.unwrap_or_else(|| {
        world
            .spawn((
                Name::new("Slow motion: the edges darkened"),
                Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() },
                Visibility::Hidden,
                ZIndex(0),
            ))
            .id()
    });
    s.shade = Some(e);
    let a = SHADE * level;
    let gradient = RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestCorner,
        vec![ColorStop::percent(Color::srgba(0., 0., 0., 0.), 38.), ColorStop::percent(Color::srgba(0.02, 0.02, 0.03, a), 100.)],
    );
    world.entity_mut(e).insert((BackgroundGradient::from(gradient), Visibility::Visible));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One kill: a quarter pace for about 0.6 seconds of real time, eased in and out, and back to 1.
    #[test]
    fn a_kill_slows_time_to_a_quarter_for_a_moment_and_eases_back() {
        let mut s = SlowMo::default();
        assert_eq!(s.scale(3.), 1.);
        assert!(s.kill(10., false, None));
        assert_eq!(s.scale(9.99), 1., "not before it");
        assert!(s.scale(10.03) < 1. && s.scale(10.03) > SLOW, "easing in");
        for t in [10.07, 10.3, 10.59] {
            assert_eq!(s.scale(t), SLOW, "{t}");
        }
        let out = [10.65, 10.8, 10.95];
        assert!(out.windows(2).all(|w| s.scale(w[0]) < s.scale(w[1])), "easing back");
        assert_eq!(s.scale(10. + HOLD + EASE_OUT), 1.);
        assert_eq!(s.level(10.3), 1.);
        assert_eq!(s.level(11.5), 0.);
        // Its own pace for the round offline; online it is the screen's alone.
        assert_eq!(s.sim_scale(10.3), SLOW);
        let mut online = SlowMo::default();
        online.kill(10., true, None);
        assert_eq!((online.scale(10.3), online.sim_scale(10.3)), (SLOW, 1.));
    }

    /// Kills in a row hold it longer, never past CAP; once over, another must wait REST.
    #[test]
    fn a_chain_of_kills_holds_it_longer_but_never_past_the_cap() {
        let mut s = SlowMo::default();
        s.kill(0., false, None);
        for t in [0.3, 0.5, 0.7, 0.9, 1.1, 1.3, 1.45] {
            assert!(s.kill(t, false, None), "{t}");
            assert!(s.hold <= CAP + 1e-6, "{t}: {}", s.hold);
        }
        assert!((s.hold - CAP).abs() < 1e-6);
        assert_eq!(s.scale(CAP - 0.01), SLOW);
        assert!(s.scale(CAP + 0.1) > SLOW);
        assert_eq!(s.scale(CAP + EASE_OUT), 1., "over");
        // In the rest after it, a kill starts nothing.
        assert!(!s.kill(CAP + EASE_OUT + 0.2, false, None));
        assert_eq!(s.scale(CAP + EASE_OUT + 0.3), 1.);
        // After it, a new one.
        let later = CAP + EASE_OUT + REST + 0.1;
        assert!(s.kill(later, false, Some(Vec3::X)));
        assert_eq!((s.hold, s.scale(later + 0.3), s.focus), (HOLD, SLOW, Some(Vec3::X)));
    }
}

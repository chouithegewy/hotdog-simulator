// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The demo's camera on a Titan map (`?demo&map=titans-kitchen`, natively `--demo` with one).
//!
//! The demo's orbit over a counter (arena.rs `spectator`) is framed for a counter 3.6 m long: on the kitchen floor it
//! circles out through the room's walls and under its furniture, and spent its time looking at the underside of a
//! table. Here it frames the fight instead:
//!
//! * **The giant**, by default, in its own third-person framing (titan/camera.rs `behind`: back and above it, the
//!   whole giant and the floor round it in view, pulled in before a wall), eased as it turns.
//! * **A small hotdog**, cut to only when something happens to it ([`news`]): the giant hits it or kills it. The camera
//!   watches it from low behind for [`CUT`] seconds, placed as the death camera places itself (deathcam.rs `settle`):
//!   never inside a den's roof, but low and looking out of the den's mouth when it hides under the table or the
//!   sideboard; then it cuts back to the giant, and holds there at least [`HOLD`] seconds before the next cut.
//!
//! Only the demo (the address's `demo`, natively `--demo`), and only on a Titan round; a replay has its own cameras.
use crate::arena::Game;
use crate::deathcam::Orbit;
use crate::visuals::ArenaBounds;
use bevy::prelude::*;
use kitchen_sim::feast::Player;
use std::collections::HashMap;

/// Seconds the camera stays on a small hotdog something happened to, and the least it then stays on the giant.
pub const CUT: f32 = 3.;
pub const HOLD: f32 = 5.;
/// How fast the camera eases after what it watches (a rate, per second), and its field of view (degrees).
const EASE: f32 = 4.;
const FOV: f32 = 62.;
/// Low behind a small hotdog: how steeply it looks down and from how far (as the death camera's [`Orbit`]).
const SMALL: Orbit = Orbit { rise: 0.42, reach: 0.7 };
/// A record older than this (the round's clock) is not news.
const FRESH: f32 = 1.;

/// A record as it was last seen: kind, attacker, time; and whether the hotdog was alive.
type Seen = ((u8, u32, u32), bool);

#[derive(Resource, Default)]
pub struct Director {
    /// The small hotdog watched, and until when (the round's clock); None: the giant.
    pub on: Option<(u32, f32)>,
    /// When the camera last came back to the giant (the round's clock).
    pub back: f32,
    seen: HashMap<u32, Seen>,
    /// Where the camera is and what it looks at, as eased; and whom that was.
    shot: Option<(Vec3, Vec3, Option<u32>)>,
}
impl Director {
    /// Whom the camera watches at `now` (the round's clock), with `happened` (small hotdogs the giant just hit or
    /// killed, [`news`]): a cut to one of them if the camera has been on the giant HOLD seconds, back to the giant CUT
    /// seconds later (or at once if the one watched has left the round).
    pub fn choose(&mut self, happened: &[u32], roster: &[Player], now: f32) -> Option<u32> {
        if let Some((id, until)) = self.on {
            if now < until && roster.iter().any(|p| p.id == id) {
                return Some(id);
            }
            (self.on, self.back) = (None, now);
        }
        if now - self.back >= HOLD
            && let Some(&id) = happened.first()
        {
            self.on = Some((id, now + CUT));
            return Some(id);
        }
        None
    }
}

/// The small hotdogs (all but `titan`) that the giant has hit or killed since the last look (`seen`, which this moves
/// on), at the round's clock `elapsed`. A hotdog seen for the first time tells nothing.
pub fn news(seen: &mut HashMap<u32, Seen>, roster: &[Player], titan: u32, elapsed: f32) -> Vec<u32> {
    let mut out = vec![];
    for p in roster.iter().filter(|p| p.id != titan) {
        let d = p.last_damage;
        let now = ((d.kind, d.attacker, d.time.to_bits()), p.alive);
        let Some((was, alive)) = seen.insert(p.id, now) else { continue };
        let hit = was != now.0 && d.attacker == titan && d.amount > 0. && (elapsed - d.time).abs() <= FRESH;
        let killed = alive && !p.alive && d.attacker == titan;
        if hit || killed {
            out.push(p.id);
        }
    }
    seen.retain(|id, _| roster.iter().any(|p| p.id == *id));
    out
}

/// Where the camera stands to watch small hotdog `p` from low behind it, on `bounds` among `scene` (as the death
/// camera stands: out of every roof and wall), and the point it looks at.
pub fn small_shot(p: &Player, bounds: &ArenaBounds, scene: &crate::ragdoll::Scene) -> (Vec3, Vec3) {
    let body = Vec3::from_array(p.p) + Vec3::Y * if p.alive { 0.11 } else { 0.03 };
    let (angle, back, reach) = crate::deathcam::settle(body, p.yaw, SMALL, bounds, scene);
    (crate::deathcam::place(body, angle, back, reach, bounds), body)
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Director>().add_systems(Update, film.after(crate::arena::tick).before(crate::slowmo::present));
    }
}

/// After the round's tick (which put the demo's camera on its orbit): on a Titan round, the giant or the small hotdog
/// something has just happened to.
fn film(world: &mut World) {
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    let Some(mut d) = world.remove_resource::<Director>() else { return };
    let dt = world.get_resource::<Time<Real>>().map_or(0., |t| t.delta_secs().min(0.1));
    let wanted = world.get_resource::<Game>().filter(|g| demo && g.playing()).and_then(|g| {
        let titan = crate::titan::titan_id(g)?;
        let happened = news(&mut d.seen, &g.roster, titan, g.elapsed);
        let whom = d.choose(&happened, &g.roster, g.elapsed);
        let shot = match whom.and_then(|id| g.roster.iter().find(|p| p.id == id)) {
            Some(p) => {
                let bounds = world.get_resource::<ArenaBounds>().copied().unwrap_or_default();
                let empty = crate::ragdoll::Scene::default();
                let scene = world.get_resource::<crate::death::Deaths>().map_or(&empty, |deaths| &deaths.scene);
                let (eye, at) = small_shot(p, &bounds, scene);
                (eye, at, Some(p.id))
            }
            None => {
                let giant = g.roster.iter().find(|p| p.id == titan)?;
                let framed = crate::titan::camera::behind(&g.feast.level, Vec3::from_array(giant.p), giant.yaw, 0.);
                (framed.translation, framed.translation + framed.forward() * 3., None)
            }
        };
        Some((g.camera()?, shot))
    });
    let Some((cam, (eye, at, whom))) = wanted else {
        (d.on, d.shot) = (None, None);
        if !demo {
            d.seen.clear();
        }
        world.insert_resource(d);
        return;
    };
    // Eased after what it watches; a cut is a cut.
    let k = 1. - (-dt * EASE).exp();
    let (eye, at) = match d.shot {
        Some((was_eye, was_at, was)) if was == whom => (was_eye.lerp(eye, k), was_at.lerp(at, k)),
        _ => (eye, at),
    };
    d.shot = Some((eye, at, whom));
    if let Some(mut tr) = world.get_mut::<Transform>(cam) {
        *tr = Transform::from_translation(eye).looking_at(at, Vec3::Y);
    }
    if let Some(mut projection) = world.get_mut::<Projection>(cam)
        && let Projection::Perspective(p) = projection.as_mut()
    {
        p.fov = FOV.to_radians();
    }
    world.insert_resource(d);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::feast::Damage;
    use kitchen_sim::map::Map;

    fn titans() -> Map {
        Map::parse(include_str!("../assets/maps/titans-kitchen.json")).unwrap()
    }
    fn small(id: u32, at: [f32; 3], yaw: f32) -> Player {
        Player { id, p: at, yaw, alive: true, hp: 100., ..default() }
    }

    /// The giant by default; a cut to a small hotdog only when the giant hits or kills it, for CUT seconds, then back to
    /// the giant for at least HOLD; a hit by anyone else, or an old record, is no news.
    #[test]
    fn the_demo_watches_the_giant_and_cuts_to_a_small_hotdog_only_when_the_giant_gets_it() {
        let (titan, floor) = (1, kitchen_sim::map::FLOOR as f32);
        let mut roster = vec![small(1, [0., floor, -10.], 0.), small(2, [1., floor, -10.], 0.), small(3, [-1., floor, -9.], 0.)];
        let mut d = Director { back: -HOLD, ..default() };
        assert!(news(&mut d.seen, &roster, titan, 0.).is_empty(), "first seen: nothing");
        assert_eq!(d.choose(&[], &roster, 0.), None, "the giant");
        // Hotdog 3 shoots hotdog 2: no news. The giant swats 2: news, and the camera is on it.
        roster[1].last_damage = Damage { kind: 1, attacker: 3, time: 1., amount: 20., ..default() };
        assert!(news(&mut d.seen, &roster, titan, 1.).is_empty());
        roster[1].last_damage = Damage { kind: 20, attacker: titan, time: 1.5, amount: 60., ..default() };
        let happened = news(&mut d.seen, &roster, titan, 1.5);
        assert_eq!(happened, vec![2]);
        assert_eq!(d.choose(&happened, &roster, 1.5), Some(2));
        assert_eq!(d.choose(&[], &roster, 1.5 + CUT - 0.1), Some(2));
        // Back to the giant after CUT; another hit at once is not cut to until HOLD has passed.
        assert_eq!(d.choose(&[], &roster, 1.5 + CUT), None);
        roster[2].last_damage = Damage { kind: 20, attacker: titan, time: 5., amount: 60., ..default() };
        let happened = news(&mut d.seen, &roster, titan, 5.);
        assert_eq!(happened, vec![3]);
        assert_eq!(d.choose(&happened, &roster, 5.), None, "the giant holds");
        // A kill is news; HOLD later the camera goes.
        roster[2].alive = false;
        let happened = news(&mut d.seen, &roster, titan, 1.5 + CUT + HOLD);
        assert_eq!(happened, vec![3]);
        assert_eq!(d.choose(&happened, &roster, 1.5 + CUT + HOLD), Some(3));
        // The one watched leaves the round: back to the giant at once.
        roster.remove(2);
        assert_eq!(d.choose(&[], &roster, 1.5 + CUT + HOLD + 0.5), None);
    }

    /// A small hotdog under the table, under the sideboard, under a chair, in the open: the camera that watches it is
    /// never inside a roof (nor any other block) and sees it; under a roof it stands below it.
    #[test]
    fn a_small_hotdog_in_a_den_is_watched_from_under_its_roof_never_from_inside_it() {
        let map = titans();
        let scene = crate::death::scenery(&map);
        let floor = kitchen_sim::map::FLOOR as f32;
        let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
        let bounds = ArenaBounds { min: Vec2::new(x0, z0), max: Vec2::new(x1, z1), floor, ceiling: floor + crate::floor::HEIGHT, room: true };
        // (Under the table top at 0.72 m, under the sideboard at 0.22 m, under a chair's seat at 0.43 m, in the open.)
        let dens = [([-1.3, -9.9], 0.72), ([-3.45, -10.4], 0.22), ([-1.65, -9.25], 0.43), ([1.0, -9.0], f32::INFINITY)];
        for ([x, z], roof) in dens {
            for k in 0..8 {
                let p = small(2, [x, floor, z], k as f32 * std::f32::consts::TAU / 8.);
                let (eye, at) = small_shot(&p, &bounds, &scene);
                assert!(scene.clear(eye, 0.02, 0.), "({x}, {z}) yaw {k}: the camera at {eye} is inside something");
                assert!(eye.y < floor + roof - 0.02, "({x}, {z}) yaw {k}: {} over the floor, the roof {roof}", eye.y - floor);
                assert!((1..10).all(|i| scene.clear(eye.lerp(at, i as f32 * 0.1), 0.005, 0.)), "({x}, {z}) yaw {k}: it sees the hotdog from {eye}");
            }
        }
        // A replay's follow camera (behind and over the hotdog, 0.6 m off) is pulled in under the table top, not left
        // in it or over it; in the open it stands where it was asked to.
        let (at, over) = (Vec3::new(-1.3, floor + 0.09, -9.9), Vec3::new(-1.3, floor + 0.5, -9.5));
        let wanted = at + (over - at).normalize() * 0.6;
        let eye = crate::deathcam::pull_in(&scene, at, wanted, 0.03);
        assert!(eye.y < floor + 0.72 - 0.02 && scene.clear(eye, 0.02, 0.) && eye.distance(at) > 0.05, "{eye}");
        let open = Vec3::new(1., floor + 0.09, -9.);
        let wanted = open + Vec3::new(0., 0.3, 0.5);
        assert!(crate::deathcam::pull_in(&scene, open, wanted, 0.03).distance(wanted) < 1e-4);
    }
}

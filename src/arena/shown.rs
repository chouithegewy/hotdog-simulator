// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What this player's own screen shows of the first person, read for its recorded view (replay/pov.rs) and put back
//! when a replay is watched through that view: the parts of `Game` the presentation reads that the arena keeps to
//! itself (the kill cross, the stim clap, the eye, the rifle's burst as its optic counts it), the first-person guns
//! built under the camera of a viewer that has none, and the crosshair and the sniper's scope drawn again once the
//! state is put back. Nothing here is a second way of drawing anything: the same code draws it from the same state.
use super::*;

/// The private parts of the first person a recorded view keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Private {
    /// The kill cross (seconds left), seconds into the stim clap, the eye's height over the feet as it eases.
    pub hit: f32,
    pub clap: f32,
    pub eye: f32,
}
impl Game {
    /// What the recorder reads of the private parts.
    pub(crate) fn private(&self) -> Private {
        Private { hit: self.hit, clap: self.clap, eye: self.eye }
    }
    /// A recorded view puts them back, with the rifle's burst as its optic counted it then (ready of size): no round of
    /// a burst is fired here, nothing is heard; `burst` says it to the optic.
    pub(crate) fn put_back(&mut self, private: Private, burst: (u32, u32)) {
        (self.hit, self.clap, self.eye) = (private.hit, private.clap, private.eye);
        let (ready, size) = burst;
        (self.volley, self.volley_wait, self.cooldown) = if ready >= size || size == 0 {
            (0, 0., 0.)
        } else if ready == 0 {
            (0, 0., 1.)
        } else {
            (ready, f32::MAX, 0.)
        };
    }
    /// The stim clap `clap` seconds in after a frame of `dt` seconds: what to put back before the arena's tick, which
    /// moves it on by its frame and poses the forearm from it.
    pub(crate) fn clap_before(&mut self, clap: f32, dt: f32) {
        self.clap = clap - dt;
    }
    /// The reconstructed view (no recorded view of the watched player): a shot of its that the recording tells of
    /// kicks the gun in hand as a shot of its own would (`shoot`), and a swing begins as its own (`strike`).
    pub(crate) fn kick_shown(&mut self, weapon: Weapon) {
        let w = weapon as usize;
        if w == 0 || w >= feast::GUNS {
            return;
        }
        let recoil = self.feast.tune.recoil[w];
        self.kick = if weapon == Weapon::Pistol { (self.kick + recoil).min(PISTOL_STACK * recoil.max(1.)) } else { recoil };
        self.spring.shot();
    }
    pub(crate) fn swing_shown(&mut self, weapon: Weapon) {
        let elapsed = self.elapsed;
        self.whip.start(weapon, elapsed);
    }
    /// The stim's seconds left on this player's hotdog, as the arena shows its bolts.
    pub(crate) fn stim_left(&self) -> f32 {
        self.roster.iter().find(|p| p.id == self.me() && p.alive).map_or(0., |p| p.stim)
    }
}
/// The first-person guns (and the stim's forearm and bolts) built under the game's camera if they are not yet: a
/// replay's viewer is watched, and the arena builds none for it. Whether there are some now.
pub(crate) fn ensure_viewmodels(world: &mut World) -> bool {
    let Some(v) = world.get_resource::<Visuals>().cloned() else { return false };
    if world.get_resource::<crate::player_model::PlayerModels>().is_none() {
        return false;
    }
    world.resource_scope(|world, mut g: Mut<Game>| {
        if !g.viewmodels.is_empty() {
            return true;
        }
        let Some(cam) = g.camera.or_else(|| world.query_filtered::<Entity, With<crate::maya::HotdogCamera>>().iter(world).next()) else { return false };
        g.camera = Some(cam);
        let (viewmodels, stim_arm, bolts) = build_viewmodels(world, &v, cam);
        g.viewmodels = viewmodels;
        g.stim_arm = Some(stim_arm);
        g.bolts = bolts;
        true
    })
}
/// The crosshair, the sniper's scope and the words of the display as the game's state says now (the arena's own
/// `update_hud`), drawn again after a recorded view has put the state back.
pub(crate) fn redraw_hud(world: &mut World) {
    world.resource_scope(|world, g: Mut<Game>| update_hud(world, &g));
}

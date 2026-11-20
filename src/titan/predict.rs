// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Online, the titan's player predicts its own giant as the server moves it (kitchen_sim titan.rs: `titan_command`,
//! `titan_drive`, `titan_charge`, `titan_air`), not as a hotdog: its body turns towards the look at `TURN` (slower in a
//! move's wind-up and recovery, not at all in a reel or its run), walks only when free, at its own pace (slower backing
//! or sidling, `stride_pace`), slid along whatever its footprint meets (`Footing::slide`); in a charge's run it goes
//! straight ahead at `CHARGE_SPEED`, steering a little; in the air it flies as it took off. What the server says of its
//! move (the view's move, when it began, how long it lasts) and of where it was is the starting point; each command
//! since is foreseen from it (as Kung Fu's `Foresight` does, fight.rs). arena/online.rs's `Predictor` keeps and corrects
//! the prediction as it does a hotdog's.
use bevy::prelude::*;
use kitchen_sim::actor::Actor;
use kitchen_sim::net::{Command, button};
use kitchen_sim::titan::{self as sim, Footing, Kind, Move};

/// What the client knows of its own titan when it predicts it.
#[derive(Clone, Debug)]
pub struct Foresight {
    /// Where its footprint may go (the level's).
    pub footing: Footing,
    /// Its body's yaw and its walk (m/s, flat) when the server last said; in a jump, the walk it took off with.
    pub yaw: f32,
    pub walk: Vec2,
    /// Its move: which, when it began (round clock), how long it lasts (0: a stance loop).
    pub mv: Move,
    pub started: f32,
    pub len: f32,
    /// The newest frame's ack and round clock (online.rs `Online::clock`): command `ack` was applied at `elapsed`.
    pub clock: (u32, f32),
}
impl Foresight {
    /// The titan's player's foresight from what the game knows now (None unless this player is the titan).
    pub fn of(g: &crate::arena::Game) -> Option<Foresight> {
        let me = g.me();
        if super::titan_id(g) != Some(me) {
            return None;
        }
        let view = g.titan.as_ref()?;
        let p = g.online.latest.iter().find(|p| p.id == me).or_else(|| g.roster.iter().find(|p| p.id == me))?;
        // (A jump flies with the walk it had as it began: the client's own, kept as it stood free.)
        let walk = if view.mv() == Move::Jump { g.online.titan_run_up } else { Vec2::new(p.v[0], p.v[2]) };
        Some(Foresight { footing: Footing::of(&g.feast.level), yaw: p.yaw, walk, mv: view.mv(), started: view.started, len: view.len, clock: g.online.clock })
    }
    /// The round's clock when command `seq` is applied (`dt` a step): `ack`'s step ended at the clock the server gave.
    fn time(&self, seq: u32, dt: f32) -> f32 {
        self.clock.1 + (seq as f32 - self.clock.0 as f32 - 1.) * dt
    }
    /// Its move's phase at round time `t`: free (a stance), in its wind-up (0), active (1), recovering (2).
    fn phase(&self, t: f32) -> Option<u8> {
        let tt = t - self.started;
        if matches!(self.mv, Move::Idle | Move::Walk) || (self.len > 0. && tt >= self.len) {
            return None;
        }
        let d = self.mv.data();
        let active_end = if self.len > 0. { self.len - d.recovery } else { d.startup + d.active };
        Some(if tt < d.startup { 0 } else if tt < active_end { 1 } else { 2 })
    }
    /// How fast it turns at round time `t` (titan.rs `titan_drive`, `titan_charge`).
    fn rate(&self, t: f32, airborne: bool) -> f32 {
        if airborne {
            return 0.;
        }
        match self.phase(t) {
            None => sim::TURN,
            Some(1) if self.mv == Move::Charge => sim::CHARGE_TURN,
            Some(_) if matches!(self.mv.data().kind, Kind::Reel | Kind::Down | Kind::Victory | Kind::Charge) => 0.,
            Some(0) => sim::TURN * 0.5,
            Some(2) => sim::TURN * 0.3,
            _ => 0.,
        }
    }
    /// Its body's yaw after the commands `before` (those after the server's word), each `dt` long.
    pub fn yaw_after(&self, before: &[Command], dt: f32, airborne: bool) -> f32 {
        before.iter().filter(|b| b.seq > self.clock.0).fold(self.yaw, |yaw, b| sim::turn(yaw, b.yaw(), dt, self.rate(self.time(b.seq, dt), airborne)))
    }
    /// The body after command `c`, sent after `before`: turned, walked (or run, or flown), as the server will.
    pub fn step(&self, body: &mut Actor, c: &Command, before: &[Command], dt: f32) {
        let t = self.time(c.seq, dt);
        // A jump takes off as its wind-up ends (titan.rs `titan_active`), whether or not the server has said so yet.
        if self.mv == Move::Jump && body.grounded {
            let off = t - self.started - Move::Jump.data().startup;
            if (0. ..dt * 1.5).contains(&off) {
                (body.grounded, body.v[1]) = (false, sim::JUMP_SPEED as f64);
            }
        }
        let airborne = !body.grounded;
        let yaw = sim::turn(self.yaw_after(before, dt, airborne), c.yaw(), dt, self.rate(t, airborne));
        let from = Vec2::new(body.p[0] as f32, body.p[2] as f32);
        let to = if airborne {
            // Flying as it took off: its walk carried on, under the game's gravity.
            let g = kitchen_sim::actor::GRAVITY;
            let mut y = body.p[1] + body.v[1] * dt as f64 - 0.5 * g * (dt * dt) as f64;
            body.v[1] -= g * dt as f64;
            if y <= kitchen_sim::feast::FLOOR as f64 {
                (y, body.v[1], body.grounded) = (kitchen_sim::feast::FLOOR as f64, 0., true);
            }
            body.p[1] = y;
            self.footing.slide(from, self.walk * dt).0
        } else {
            match self.phase(t) {
                // Free: the stick walks it, at its pace (crouched slower, backing and sidling slower still).
                None => {
                    let way = c.stride(1., 1.);
                    let share = way.length().min(1.);
                    if share < 0.01 {
                        from
                    } else {
                        let dir = way / way.length();
                        let crouch = c.has(button::CROUCH);
                        let pace = if crouch { sim::CROUCH_WALK } else { sim::WALK };
                        self.footing.slide(from, dir * sim::stride_pace(pace, yaw, dir) * share * dt).0
                    }
                }
                // A charge's run: straight ahead, steering a little; into a wall, it stops there (and stumbles).
                Some(1) if self.mv == Move::Charge => {
                    let ahead = sim::facing(yaw);
                    let (to, stopped) = self.footing.slide(from, ahead * sim::CHARGE_SPEED * dt);
                    if stopped && (to - from).dot(ahead) < sim::CHARGE_SPEED * dt * 0.5 { from } else { to }
                }
                _ => from,
            }
        };
        (body.p[0], body.p[2]) = (to.x as f64, to.y as f64);
    }
}

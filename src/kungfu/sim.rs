// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What the animation is told of each fighter, from the simulation's word (`kitchen_sim::kungfu::View`: offline the
//! feast's own, online the server's `{type:'kungfu'}` message; arena.rs keeps it as `Game::kungfu`).
use super::fu::{self, Kind, Move};
use super::{Timing, v};
use bevy::prelude::*;

/// One fighter, as the animation needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fighter {
    pub id: u32,
    /// Its style (`fu::Style` as its number).
    pub style: usize,
    /// The move it is in, or None between moves (in its stance, walking).
    pub action: Option<Move>,
    /// Seconds into the move.
    pub t: f32,
    /// The move's timing this time (the table's, scaled for a Drunken strike, a reaction as long as its stun).
    pub timing: Timing,
    /// The move has landed (a hit, a block or a grab's catch).
    pub connected: bool,
    /// The move is held by the simulation (a block).
    pub held: bool,
    /// How far the simulation has carried the hotdog through the move so far (right, up, forward), and will have by
    /// its end: the animation's keys are written from where it began.
    pub carried: Vec3,
    /// Alive (a knocked-out hotdog is the ragdoll's), and off the ground (jumping, thrown).
    pub alive: bool,
    pub airborne: bool,
}
impl Default for Fighter {
    fn default() -> Self {
        Fighter { id: 0, style: 0, action: None, t: 0., timing: Timing::default(), connected: false, held: false, carried: Vec3::ZERO, alive: true, airborne: false }
    }
}
/// The timing of `mv` stretched by `scale` (a Drunken strike's), and lasting `len` seconds in all when that is given (a
/// reaction lasts as long as its stun).
pub fn timing(mv: Move, scale: f32, len: f32) -> Timing {
    let d = mv.data();
    let scale = if scale > 0. { scale } else { 1. };
    let (startup, active) = (d.startup * scale, d.active * scale);
    let mut recovery = d.recovery * scale;
    if len > 0. && (len - startup - active - recovery).abs() > 1e-3 {
        recovery = (len - startup - active).max(0.02);
    }
    Timing { startup, active, recovery }
}
/// The way the simulation carries a hotdog through `mv` (right, up, forward), or zero.
pub fn way(mv: Move) -> Vec3 {
    match mv {
        Move::DodgeLeft => v(-1., 0., 0.),
        Move::DodgeRight => v(1., 0., 0.),
        Move::BackHop => v(0., 0., -1.),
        _ => v(0., 0., 1.),
    }
}
/// How far the simulation carries a hotdog fighting in `style` through `mv` over its startup and active window, at
/// its own (unscaled) timing: the table's `advance`, longer for a Monkey's dodges.
pub fn advance(style: usize, mv: Move) -> f32 {
    let d = mv.data();
    let monkey = style == fu::Style::Monkey.index() && d.kind == Kind::Dodge;
    d.advance * if monkey { fu::DODGE_MONKEY } else { 1. }
}
/// The whole of a move's carry, as written in its keys (right, up, forward).
pub fn travel(style: usize, mv: Move) -> Vec3 {
    way(mv) * advance(style, mv)
}
/// How far the simulation has carried the hotdog `t` seconds into a move timed `timing` that carries it `whole`: evenly
/// over its startup and active window.
pub fn carried(whole: Vec3, timing: Timing, t: f32) -> Vec3 {
    let span = (timing.startup + timing.active).max(1e-3);
    whole * (t / span).clamp(0., 1.)
}
/// A fighter from the simulation's view of it at the round's clock `elapsed`.
pub fn fighter(f: &fu::FighterView, elapsed: f32) -> Fighter {
    let mv = f.mv();
    let style = (f.style as usize).min(fu::STYLES - 1);
    let kind = mv.data().kind;
    let action = (kind != Kind::Stance).then_some(mv);
    let timing = timing(mv, f.scale, f.len);
    let mut t = f.t(elapsed);
    // The winner bows and taunts over and over.
    if kind == Kind::Victory {
        t = t.rem_euclid(timing.total().max(0.1));
    }
    Fighter {
        id: f.id,
        style,
        action,
        t,
        timing,
        connected: f.connected,
        held: kind == Kind::Block,
        carried: carried(travel(style, mv), timing, t),
        alive: true,
        airborne: false,
    }
}
/// Every fighter, and the events, when the round is Kung Fu.
pub struct Roster {
    pub fighters: Vec<Fighter>,
    pub events: Vec<fu::Event>,
    /// This player's hotdog, and how it moves (for its first-person view when it has no body: online).
    pub me: u32,
    pub moving: super::Locomotion,
    /// Where this player stands.
    pub position: Option<Vec3>,
}
/// The fight as the game knows it this frame, or None outside Kung Fu.
pub fn roster(world: &World) -> Option<Roster> {
    let g = world.get_resource::<crate::arena::Game>()?;
    let view = view(g)?;
    let elapsed = g.elapsed;
    let now = world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs_f64());
    let me = g.me();
    let fighters = view
        .fighters
        .iter()
        .map(|f| {
            let player = g.roster.iter().find(|p| p.id == f.id);
            // KF6: online each move at the clock fight.rs `clock` gives it (smooth between frames; a carrying move with its
            // body); offline `elapsed`.
            Fighter { alive: player.is_none_or(|p| p.alive), airborne: player.is_some_and(|p| !p.grounded), ..fighter(f, crate::fight::clock(g, f, now)) }
        })
        .collect();
    let moving = g.roster.iter().find(|p| p.id == me).map_or_else(super::Locomotion::default, |p| {
        let (ahead, right) = (Quat::from_rotation_y(p.yaw) * Vec3::NEG_Z, Quat::from_rotation_y(p.yaw) * Vec3::X);
        let speed = Vec3::new(p.v[0], 0., p.v[2]) / crate::player_model::RIG.3;
        super::Locomotion { forward: speed.dot(ahead), strafe: speed.dot(right), phase: elapsed * 15., crouch: p.crouch, airborne: !p.grounded }
    });
    let position = g.roster.iter().find(|p| p.id == me).map(|p| Vec3::from_array(p.p));
    Some(Roster { fighters, events: view.events.clone(), me, moving, position })
}
/// The game's copy of the fight (`Game::kungfu`, KF4's plumbing).
fn view(g: &crate::arena::Game) -> Option<&fu::View> {
    g.kungfu.as_ref()
}

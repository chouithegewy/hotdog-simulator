// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Network play (protocol 12, kitchen_sim::net): what the client does between the server's frames.
//!
//! **Its own hotdog is predicted.** Input is sampled once a step of the server's length (60 a second): each sample is
//! a numbered command (`Command`) that goes to the server and is applied at once to the client's own copy of its body,
//! with the same sums the server will do (`Predictor::step`: kitchen-sim's walk, jump and step, `Feast::predict`).
//! Each command is kept with the body as predicted after it. A frame says the last command the server has applied and
//! where the body was then (`Mine`): if the prediction for that command agrees, nothing happens; if not (somebody
//! pushed the hotdog, a blast threw it, the server refused a walk), the body is put where the server says and every
//! command since is applied again (`Predictor::reconcile`), against the props as they stand, without shoving them
//! twice. What that moves the body by is not shown at once: a small correction is eased away over a few frames, a
//! large one (SNAP) taken at once. The body is drawn between its last two steps, so that it moves smoothly at any
//! frame rate. Shots and strikes are shown and heard at once, as before; the server lands them, and the rounds come
//! back in its frames.
//!
//! **Everything else is drawn a little in the past.** Each frame's places (the other hotdogs, the cats, the rounds in
//! the air) and its splats and shots go into a `Timeline`; they are drawn at a clock that runs BEHIND frame intervals
//! behind the newest frame, between the two frames either side of it, so that they move smoothly whatever the network
//! does; past the newest frame they are carried on along their way for at most EXTRAPOLATE seconds. The clock keeps
//! to the frames' own steps, following the earliest any frame of the last second came: it is put right at once when
//! more than 0.15 s out, and otherwise runs up to a fifth fast or slow until it is where it should be.
use super::*;
use kitchen_sim::net::{self, Command, Mine, button};

/// A correction further than this is taken at once; nearer, it is eased away, EASE of what is left every second.
pub const SNAP: f32 = 0.25;
const EASE: f32 = 12.;
/// A prediction this close to what the server says (and alike in standing and being thrown) needs no correcting.
const AGREE: f64 = 0.0015;
/// The most commands one frame makes: after a long frame (a tab out of sight) the rest of the time is let go.
const MOST: usize = 15;
/// How far behind the newest frame the clock runs, in frame intervals, and the longest anything is carried on past
/// the newest frame, in seconds.
pub const BEHIND: f32 = 2.;
const EXTRAPOLATE: f64 = 0.1;

/// The commands sent and not yet applied by the server, each with the body as this client predicted it after it.
#[derive(Default)]
pub struct Predictor {
    pub pending: VecDeque<(Command, Actor)>,
    /// How often a frame disagreed with the prediction and the commands were applied again, and how often the
    /// correction was taken at once.
    pub corrections: u32,
    pub snaps: u32,
    /// How far the corrections moved the body, in all and at most (metres).
    pub moved: f32,
    pub largest: f32,
}
/// What a command's walk depends on beyond the command: the hotdog's number, the step's length, and what the client
/// knows of its stim and of what it carries.
#[derive(Clone)]
pub struct Context {
    pub me: u32,
    pub dt: f32,
    pub stim: bool,
    pub burden: f32,
    /// Kung Fu: this player's fighter as the server last told it, and the server's clock (fight.rs `Foresight`), from
    /// which each command's walk (`kungfu::pace`: none in a move) and lunge or dodge (`kungfu::lunge`) are foreseen;
    /// its PRONE is the special, not lying down. None in any other mode.
    pub kungfu: Option<crate::fight::Foresight>,
}
impl Predictor {
    /// Applies a command to the client's own body (in `ghosts`) as the server will (crates/server game.rs `command`):
    /// the stances, the walk at the pace they and the stim and the load allow (a quarter of it while thrown), the
    /// jump, the step. Keeps it, with the body after it. Returns where the body now stands, or None without one.
    pub fn step(&mut self, feast: &mut Feast, ghosts: &mut actor::World, c: &Command, ctx: &Context) -> Option<Vec3> {
        // (Kung Fu foresees a command's step from those before it: fight.rs.)
        let before: Vec<Command> = if ctx.kungfu.is_some() { self.pending.iter().map(|(c, _)| *c).collect() } else { vec![] };
        let after = apply(feast, ghosts, c, ctx, &before, true)?;
        self.pending.push_back((*c, after.clone()));
        Some(after.position())
    }
    /// A frame says the server has applied the commands up to `ack` and that the body was `mine` afterwards. When the
    /// prediction for `ack` agrees, nothing changes; otherwise the body is put where the server says and the commands
    /// since are applied again. Returns how far that moved the body (now minus before), or None when nothing moved.
    pub fn reconcile(&mut self, feast: &mut Feast, ghosts: &mut actor::World, ack: u32, mine: &Mine, ctx: &Context) -> Option<Vec3> {
        // The command acknowledged stays first until a later one is (the next frame may say the same).
        while self.pending.front().is_some_and(|(c, _)| c.seq < ack) {
            self.pending.pop_front();
        }
        let body = ghosts.get_mut(ctx.me)?;
        let acked = self.pending.front_mut().filter(|(c, _)| c.seq == ack);
        if acked.as_ref().is_some_and(|(_, p)| agrees(p, mine)) {
            return None;
        }
        let before = body.position();
        mine.onto(body);
        if let Some((_, after)) = acked {
            *after = body.clone();
        }
        let all: Vec<Command> = self.pending.iter().map(|(c, _)| *c).collect();
        let first = all.iter().position(|c| c.seq > ack).unwrap_or(all.len());
        let afters = feast.rehearse(ctx.me, |feast| (first..all.len()).filter_map(|k| apply(feast, ghosts, &all[k], ctx, &all[..k], false)).collect::<Vec<_>>());
        for ((_, after), redone) in self.pending.iter_mut().filter(|(c, _)| c.seq > ack).zip(afters) {
            *after = redone;
        }
        self.corrections += 1;
        let now = ghosts.get(ctx.me)?.position();
        let moved = (now - before).length();
        (self.moved, self.largest) = (self.moved + moved, self.largest.max(moved));
        Some(now - before)
    }
}
/// One command applied to the body `ctx.me` in `ghosts` (see `Predictor::step`), the commands sent before it being
/// `before` (oldest first: Kung Fu foresees the fighter from them); `props` says whether the props move in the step
/// too. Returns the body afterwards.
fn apply(feast: &mut Feast, ghosts: &mut actor::World, c: &Command, ctx: &Context, before: &[Command], props: bool) -> Option<Actor> {
    let body = ghosts.get(ctx.me)?;
    let (prone, crouch) = (c.has(button::PRONE) && ctx.kungfu.is_none(), c.has(button::CROUCH) && (!c.has(button::PRONE) || ctx.kungfu.is_some()));
    let control = if body.launched { ghosts.config.control as f32 } else { 1. };
    // (At the pace and the jump the server's tuning has: kitchen_sim::tune; in Kung Fu the share of it its fighter's
    // move allows, as `Feast::kungfu_command` walks, and the lunge or the dodge the move carries it by: fight.rs.)
    let (step, may_jump) = match &ctx.kungfu {
        Some(fu) => {
            let s = fu.step(c, before, ctx.dt);
            (c.stride(feast.tune.pace(false, crouch, false, 1.) * s.pace * control, ctx.dt) + s.lunge, s.free)
        }
        None => (c.stride(feast.tune.pace(prone, crouch, ctx.stim, ctx.burden) * control, ctx.dt), true),
    };
    let jump = if c.has(button::JUMP) && !prone && may_jump { feast.tune.leap(ctx.stim) as f64 } else { 0. };
    feast.predict(ghosts, ctx.me, (prone, crouch), step, jump, ctx.dt as f64, props);
    ghosts.get(ctx.me).cloned()
}
/// Whether a predicted body is where the server says, standing or not and thrown or not alike.
fn agrees(p: &Actor, m: &Mine) -> bool {
    let far = (0..3).map(|k| (p.p[k] - m.p[k] as f64).powi(2)).sum::<f64>().sqrt();
    far < AGREE && p.grounded == m.grounded && p.launched == m.launched && (p.v[1] - m.vy as f64).abs() < 0.05 && (0..2).all(|k| (p.knock[k] - m.knock[k] as f64).abs() < 0.05)
}

/// What one frame says of the things drawn in the past.
#[derive(Clone, Default)]
pub struct Moment {
    pub tick: u32,
    /// Each other hotdog: its number, place, yaw, pitch and velocity.
    pub players: Vec<(u32, Vec3, f32, f32, Vec3)>,
    /// Each cat: its number, place and yaw.
    pub enemies: Vec<(u32, Vec3, f32)>,
    pub rounds: Vec<feast::Projectile>,
    pub splats: Vec<feast::Splat>,
    pub shots: Vec<feast::Shot>,
    released: bool,
}
/// The frames' places by step, and the clock they are drawn at (in the server's steps).
#[derive(Default)]
pub struct Timeline {
    pub moments: VecDeque<Moment>,
    pub clock: f64,
    /// Each frame's step less the time it came (in steps), for the frames of the last WINDOW seconds, and the largest of
    /// them: the earliest frames have come lately, which the clock keeps to.
    arrivals: VecDeque<(f64, f64)>,
    offset: f64,
    /// A replay (replay/) drives the clock itself, in steps, at the speed it is watched at; None online.
    pub driven: Option<f64>,
    /// A replay's seek plays the frames since its keyframe at once: the shots and splats of those before this step
    /// are passed over, not shown all together.
    pub hush: u32,
}
/// How long an early frame counts for the clock.
const WINDOW: f64 = 1.;
/// Where something was at the clock, and how it was moving.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub p: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}
impl Timeline {
    /// A frame came at `now` (seconds): its moment is kept, in order (one no newer than the newest is not).
    pub fn push(&mut self, moment: Moment, now: f64, hz: f32) {
        if self.moments.back().is_some_and(|m| m.tick >= moment.tick) {
            return;
        }
        if self.moments.is_empty() {
            self.arrivals.clear();
        }
        self.arrivals.push_back((now, moment.tick as f64 - now * hz as f64));
        while self.arrivals.front().is_some_and(|(at, _)| *at < now - WINDOW) {
            self.arrivals.pop_front();
        }
        self.offset = self.arrivals.iter().map(|(_, o)| *o).fold(f64::MIN, f64::max);
        self.moments.push_back(moment);
    }
    /// Forgets everything (a new round, a new map).
    pub fn clear(&mut self) {
        self.moments.clear();
    }
    /// Moves the clock on by `dt` seconds of the frame, towards `delay` steps behind where the newest frames are at
    /// `now`; returns the shots and splats it passes, to be shown now.
    pub fn advance(&mut self, now: f64, dt: f32, hz: f32, delay: f32) -> (Vec<feast::Shot>, Vec<feast::Splat>) {
        let hz = hz as f64;
        let target = now * hz + self.offset - delay as f64;
        let behind = target - self.clock;
        if let Some(clock) = self.driven {
            self.clock = clock;
        } else if behind.abs() > hz * 0.15 {
            self.clock = target;
        } else {
            self.clock += dt as f64 * hz * (1. + (behind / hz * 4.).clamp(-0.2, 0.2));
        }
        let (mut shots, mut splats) = (vec![], vec![]);
        let hush = self.hush;
        for m in self.moments.iter_mut().filter(|m| !m.released && m.tick as f64 <= self.clock) {
            m.released = true;
            if m.tick < hush {
                continue;
            }
            shots.extend(m.shots.drain(..));
            splats.extend(m.splats.drain(..));
        }
        // Moments before the one at the clock are no longer needed.
        while self.moments.len() > 2 && self.moments[1].tick as f64 <= self.clock {
            self.moments.pop_front();
        }
        (shots, splats)
    }
    /// The two moments either side of the clock (the second None when the clock is past the newest), and how far
    /// between them it is in steps from the first.
    fn bracket(&self) -> Option<(&Moment, Option<&Moment>, f64)> {
        let i = self.moments.iter().rposition(|m| m.tick as f64 <= self.clock).unwrap_or(0);
        let a = self.moments.get(i)?;
        Some((a, self.moments.get(i + 1).filter(|b| b.tick as f64 > self.clock), (self.clock - a.tick as f64).max(0.)))
    }
    /// Another hotdog at the clock: between its places either side, or carried on along its velocity past the newest.
    pub fn player(&self, id: u32, hz: f32) -> Option<Sample> {
        let (a, b, since) = self.bracket()?;
        let pa = a.players.iter().find(|p| p.0 == id)?;
        match b.and_then(|b| Some((b.tick, b.players.iter().find(|p| p.0 == id)?))) {
            Some((tb, pb)) => {
                let t = (since / (tb - a.tick) as f64) as f32;
                Some(Sample { p: pa.1.lerp(pb.1, t), yaw: turn(pa.2, pb.2, t), pitch: pa.3 + (pb.3 - pa.3) * t })
            }
            None => Some(Sample { p: pa.1 + pa.4 * (since / hz as f64).min(EXTRAPOLATE) as f32, yaw: pa.2, pitch: pa.3 }),
        }
    }
    /// A cat at the clock (held where it was last seen past the newest).
    pub fn enemy(&self, id: u32) -> Option<Sample> {
        let (a, b, since) = self.bracket()?;
        let ea = a.enemies.iter().find(|e| e.0 == id)?;
        match b.and_then(|b| Some((b.tick, b.enemies.iter().find(|e| e.0 == id)?))) {
            Some((tb, eb)) => {
                let t = (since / (tb - a.tick) as f64) as f32;
                Some(Sample { p: ea.1.lerp(eb.1, t), yaw: turn(ea.2, eb.2, t), pitch: 0. })
            }
            None => Some(Sample { p: ea.1, yaw: ea.2, pitch: 0. }),
        }
    }
    /// The rounds in the air at the clock, each where it is then.
    pub fn rounds(&self, hz: f32) -> Vec<feast::Projectile> {
        let Some((a, b, since)) = self.bracket() else {
            return vec![];
        };
        a.rounds
            .iter()
            .map(|q| {
                let mut q = q.clone();
                let later = b.and_then(|b| Some((b.tick, b.rounds.iter().find(|r| r.id == q.id)?)));
                match later {
                    Some((tb, r)) => {
                        let t = (since / (tb - a.tick) as f64) as f32;
                        q.p = Vec3::from_array(q.p).lerp(Vec3::from_array(r.p), t).to_array();
                        q.travelled += (r.travelled - q.travelled) * t;
                        q.age += (r.age - q.age) * t;
                    }
                    None => {
                        let s = (since / hz as f64).min(EXTRAPOLATE) as f32;
                        q.p = (Vec3::from_array(q.p) + Vec3::from_array(q.v) * s).to_array();
                        q.travelled += Vec3::from_array(q.v).length() * s;
                        q.age += s;
                    }
                }
                q
            })
            .collect()
    }
}
/// A replay's seek (replay/): the next frame is taken as a new round's (everything drawn of the last is cleared, as
/// `apply_frame` does for one), and nothing is drawn from before it.
pub fn rewind(g: &mut Game) {
    g.network_round = u32::MAX;
    g.online.timeline.clear();
}
/// Part of the way from one yaw to another, the short way round.
fn turn(a: f32, b: f32, t: f32) -> f32 {
    a + ((b - a + PI).rem_euclid(TAU) - PI) * t
}

/// Network play's state in the game (see the module's documentation).
pub struct Online {
    /// Steps a second the server simulates and frames a second it sends (from its welcome).
    pub hz: f32,
    pub send_hz: f32,
    pub predictor: Predictor,
    pub timeline: Timeline,
    /// The next command's number, the commands made and not yet sent (network.rs sends them), time not yet made into
    /// a command, and what was pressed since the last command.
    pub seq: u32,
    pub unsent: Vec<Command>,
    acc: f32,
    latch: u16,
    pickup: Option<usize>,
    /// Where the predicted body stood after the last two steps (it is drawn between them), and the correction still
    /// being eased away.
    prev: Vec3,
    cur: Vec3,
    pub offset: Vec3,
    /// Seconds of stim this client has just slapped on, until a frame says so.
    stim: f32,
    /// The newest frame's hotdogs as the server has them (the ghosts are made from them).
    pub latest: Vec<Player>,
    /// The newest frame's `ack` and `elapsed`: the server's round clock just after the step that applied that command
    /// (Kung Fu's prediction foresees each command's step from it: fight.rs `Foresight`).
    pub clock: (u32, f32),
    /// When (real seconds) the newest frame came.
    pub heard: f64,
    /// Shots the clock has passed, for network.rs to show (the gun's bark, the kick, a turret's beam).
    pub shots: Vec<feast::Shot>,
}
impl Default for Online {
    fn default() -> Online {
        Online {
            hz: net::TICK_HZ as f32,
            send_hz: net::SEND_HZ as f32,
            predictor: Predictor::default(),
            timeline: Timeline::default(),
            seq: 0,
            unsent: vec![],
            acc: 0.,
            latch: 0,
            pickup: None,
            prev: Vec3::ZERO,
            cur: Vec3::ZERO,
            offset: Vec3::ZERO,
            stim: 0.,
            latest: vec![],
            clock: (0, 0.),
            heard: 0.,
            shots: vec![],
        }
    }
}
impl Online {
    /// Forgets the prediction (a new round, a new map, the lobby): the next frame starts it again.
    pub fn reset(&mut self) {
        self.predictor.pending.clear();
        self.timeline.clear();
        (self.acc, self.latch, self.pickup, self.offset, self.stim) = (0., 0, None, Vec3::ZERO, 0.);
    }
    /// The step's length.
    pub fn dt(&self) -> f32 {
        1. / self.hz
    }
    /// The server's round clock this player's predicted body is drawn at (it is drawn between its last two steps):
    /// the step of the newest command, as `clock` foresees it, less what of a step is still to be made into one.
    /// None before the first frame.
    pub fn ahead(&self) -> Option<f32> {
        let (acked, elapsed) = self.clock;
        (acked > 0).then(|| elapsed + (self.seq as f32 - acked as f32 - 1.) * self.dt() + self.acc)
    }
}

/// What this frame's input is: the stick, the yaw and pitch, and whether the trigger is held.
pub struct Input {
    pub stick: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub fire: bool,
    pub jump: bool,
}
/// Every frame of a round (arena `tick`): what was pressed is noted, and for every step of the server's length that
/// has gone by a command is made, sent and applied to the client's own body; the body is then drawn between its last
/// two steps, less what is left of a correction.
pub fn play(g: &mut Game, input: Input, frame_dt: f32) {
    let o = &mut g.online;
    let mut latch = 0;
    for (on, bit) in [(input.jump, button::JUMP), (g.pending_stim, button::STIM), (g.pending_melee, button::MELEE), (g.pending_eat, button::EAT), (g.pending_grab, button::GRAB), (g.pending_throw, button::THROW), (input.fire, button::FIRE)] {
        if on {
            latch |= bit;
        }
    }
    o.latch |= latch;
    o.pickup = o.pickup.or(g.pending_pickup);
    o.stim = (o.stim - frame_dt).max(0.);
    if g.pending_stim && g.roster.iter().any(|p| p.id == g.network_id && p.stim_wait <= 0.) {
        o.stim = g.feast.tune.stim_time;
    }
    o.acc = (o.acc + frame_dt).min(MOST as f32 * o.dt());
    let me = g.network_id;
    let roster = g.roster.iter().find(|p| p.id == me);
    let kungfu = g.fighter(me).map(|f| crate::fight::Foresight { fighter: f.clone(), clock: g.online.clock });
    let ctx = Context { me, dt: g.online.dt(), stim: kungfu.is_none() && (roster.is_some_and(|p| p.stim > 0.) || g.online.stim > 0.), burden: crate::forts::burden(g), kungfu };
    let (fu, fu_hold, fu_style) = (g.kung_fu(), g.fu_hold, g.fu_style);
    while g.online.acc >= ctx.dt {
        let o = &mut g.online;
        o.acc -= ctx.dt;
        o.seq += 1;
        let mut buttons = std::mem::take(&mut o.latch) | if input.fire { button::FIRE } else { 0 };
        let weapon = if fu {
            // Kung Fu (interface.md section 5): the held levels in every command (the block's GRAB above all, which the
            // server reads as held), the presses latched above; the weapon is the style asked for, style + 1.
            buttons = (buttons & !button::GRAB) | fu_hold;
            fu_style.map_or(0, |s| s + 1)
        } else {
            if g.prone {
                buttons |= button::PRONE;
            } else if g.crouch {
                buttons |= button::CROUCH;
            }
            // Aimed down the sights: the server halves a syrup burst's climb, and every client sees the gun at the eye.
            if g.scoped {
                buttons |= button::SCOPE;
            }
            // How it holds what it carries (the wheel turns it, X tips it), held: the server puts it down so. With its
            // hands free there is no pose (arena.rs keeps it standing), and R goes in those bits instead: both of the
            // tip's, which no pose has (`button::RELOAD`, reload.rs).
            buttons |= Command::posed(g.pose);
            if g.weapon != Weapon::Laden && std::mem::take(&mut g.mag.pending) {
                buttons = buttons & !(button::TURN | button::TIP) | button::RELOAD;
            }
            if g.weapon == Weapon::Laden { 0 } else { g.weapon as u8 }
        };
        let command = Command::new(o.seq, input.stick, input.yaw, input.pitch, buttons, weapon, o.pickup.take());
        o.unsent.push(command);
        let Game { feast, ghosts, online, .. } = g;
        if let Some(at) = online.predictor.step(feast, ghosts, &command, &ctx) {
            (online.prev, online.cur) = (online.cur, at);
        }
    }
    let o = &mut g.online;
    o.offset *= (-frame_dt * EASE).exp();
    if o.offset.length() < 1e-4 {
        o.offset = Vec3::ZERO;
    }
    if g.ghosts.get(me).is_some() {
        g.position = o.prev.lerp(o.cur, (o.acc / ctx.dt).clamp(0., 1.)) + o.offset;
    }
}
/// A frame has come: the server has applied this client's commands up to `ack`, and `mine` is its body afterwards
/// (None while it has none: dead, or between rounds). The prediction is checked against it and, if need be,
/// corrected (see `Predictor::reconcile`).
pub fn settle(g: &mut Game, ack: u32, mine: Option<Mine>) {
    let me = g.network_id;
    let Some(mine) = mine else {
        g.ghosts.remove(me);
        g.online.predictor.pending.clear();
        return;
    };
    let kungfu = g.fighter(me).map(|f| crate::fight::Foresight { fighter: f.clone(), clock: g.online.clock });
    let ctx = Context { me, dt: g.online.dt(), stim: kungfu.is_none() && (g.roster.iter().any(|p| p.id == me && p.stim > 0.) || g.online.stim > 0.), burden: crate::forts::burden(g), kungfu };
    if g.ghosts.get(me).is_none() {
        // A body to predict again: where the server has it, and nothing pending counts.
        let mut body = Actor { radius: feast::HOTDOG_RADIUS, ..Actor::hotdog(me, [0.; 3]) };
        mine.onto(&mut body);
        g.ghosts.add(body);
        g.online.predictor.pending.retain(|(c, _)| c.seq > ack);
        let at = Vec3::from_array(mine.p);
        (g.online.prev, g.online.cur, g.online.offset) = (at, at, Vec3::ZERO);
        let commands: Vec<Command> = g.online.predictor.pending.drain(..).map(|(c, _)| c).collect();
        let Game { feast, ghosts, online, .. } = g;
        for c in commands {
            if let Some(at) = online.predictor.step(feast, ghosts, &c, &ctx) {
                (online.prev, online.cur) = (at, at);
            }
        }
        g.position = g.online.cur;
        return;
    }
    let Game { feast, ghosts, online, .. } = g;
    let Some(moved) = online.predictor.reconcile(feast, ghosts, ack, &mine, &ctx) else {
        return;
    };
    let o = &mut g.online;
    o.prev += moved;
    o.cur += moved;
    if (o.offset - moved).length() > SNAP {
        o.offset = Vec3::ZERO;
        o.predictor.snaps += 1;
    } else {
        o.offset -= moved;
    }
}
/// Every frame online: the clock moves on; the other hotdogs, the cats and the rounds in the air are put where they
/// were at it (into the roster, the simulation's lists and the cats on screen); the shots and splats it passed are
/// returned to be shown.
pub fn show(world: &mut World, g: &mut Game, now: f64, frame_dt: f32) -> Vec<feast::Shot> {
    let (hz, every) = (g.online.hz, g.online.hz / g.online.send_hz.max(1.));
    let (shots, splats) = g.online.timeline.advance(now, frame_dt, hz, BEHIND * every + 1.);
    let me = g.network_id;
    let timeline = &g.online.timeline;
    for p in g.roster.iter_mut().filter(|p| p.id != me || g.demo) {
        if let Some(s) = timeline.player(p.id, hz) {
            (p.p, p.yaw, p.pitch) = (s.p.to_array(), s.yaw, s.pitch);
        }
    }
    for e in &mut g.feast.enemies {
        if let Some(s) = timeline.enemy(e.id) {
            (e.p, e.yaw) = (s.p.to_array(), s.yaw);
        }
    }
    g.feast.projectiles = timeline.rounds(hz);
    let elapsed = g.elapsed;
    g.feast.splats.retain(|s| elapsed - s.time < 0.4);
    g.feast.splats.extend(splats);
    for e in &g.enemies {
        if let Some(state) = g.feast.enemies.iter().find(|z| z.id == e.state.id) {
            if let Some(mut tr) = world.get_mut::<Transform>(e.entity) {
                tr.translation = Vec3::from_array(state.p);
                tr.rotation = Quat::from_rotation_y(state.yaw);
            }
        }
    }
    shots
}
/// The moment a frame adds to the timeline: every hotdog's place, the cats', the rounds in the air, and its shots and
/// splats.
pub fn moment(frame: &net::Frame) -> Moment {
    Moment {
        tick: frame.tick,
        players: frame.players.iter().map(|s| (s.player.id, Vec3::from_array(s.player.p), s.player.yaw, s.player.pitch, Vec3::from_array(s.player.v))).collect(),
        enemies: frame.enemies.iter().map(|e| (e.id, Vec3::from_array(e.p), e.yaw)).collect(),
        rounds: frame.projectiles.clone(),
        splats: frame.splats.clone(),
        shots: frame.shots.clone(),
        released: false,
    }
}

/// For the checks (`window.hotdogState.net`): the prediction's corrections and snaps, the commands not yet applied,
/// the correction still being eased away, how far behind the newest frame the clock draws, and the frames decoded.
pub fn report(g: &Game, net: Option<&crate::network::Network>) -> serde_json::Value {
    let o = &g.online;
    let behind = o.timeline.moments.back().map(|m| (m.tick as f64 - o.timeline.clock) / o.hz as f64 * 1000.);
    serde_json::json!({
        "corrections": o.predictor.corrections,
        "snaps": o.predictor.snaps,
        "moved": o.predictor.moved,
        "largest": o.predictor.largest,
        "pending": o.predictor.pending.len(),
        "seq": o.seq,
        "offset": o.offset.length(),
        "behind_ms": behind,
        "frames": net.map_or(0, |n| n.frames),
        "broken": net.map_or(0, |n| n.broken),
        "whole": net.map_or(0, |n| n.whole),
        "tick": net.map_or(0, |n| n.newest),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::feast::FLOOR;
    use kitchen_sim::map::Map;

    /// The server's side of a command, as crates/server's `Game::command` applies it (its walk, jump and stances), then
    /// its step.
    fn serve(s: &mut Feast, id: u32, c: &Command, dt: f32) {
        let (prone, crouch) = (c.has(button::PRONE), c.has(button::CROUCH) && !c.has(button::PRONE));
        s.set_prone(id, prone);
        s.set_crouch(id, crouch);
        let stim = s.player(id).is_some_and(|p| p.stim > 0.);
        let control = if s.body(id).is_some_and(|b| b.launched) { s.bodies.config.control as f32 } else { 1. };
        let step = c.stride(net::pace(prone, crouch, stim, s.burden(id)) * control, dt);
        s.walk(id, step, c.yaw());
        if c.has(button::JUMP) && !prone {
            s.jump(id, if stim { actor::STIM_JUMP } else { actor::JUMP } as f32);
        }
        s.tick(dt);
    }
    /// A server's round with nobody else in the way (the other hotdogs parked at the back of the counter), and the
    /// client's copy of it: its props and the ghost of its own hotdog where the server has it.
    fn pair(map: Map, at: [f32; 3]) -> (Feast, Feast, actor::World) {
        let mut server = Feast::with_map(map.clone(), false);
        for (i, p) in server.players.iter_mut().enumerate() {
            (p.bot, p.p) = (false, [-1.5 + i as f32 * 0.1, FLOOR, 3.55]);
        }
        server.players[0].p = at;
        server.tick(1. / 60.);
        let client = Feast::with_map(map, false);
        let mut ghosts = actor::World::default();
        let mut body = Actor { radius: feast::HOTDOG_RADIUS, ..Actor::hotdog(1, [0.; 3]) };
        Mine::of(server.body(1).unwrap()).onto(&mut body);
        ghosts.add(body);
        (server, client, ghosts)
    }
    fn ctx() -> Context {
        Context { me: 1, dt: 1. / 60., stim: false, burden: 1., kungfu: None }
    }
    fn walk(seq: u32, stick: Vec2, yaw: f32, buttons: u16) -> Command {
        Command::new(seq, stick, yaw, 0., buttons, 1, None)
    }
    /// Without anybody else, the prediction is the server's to the micrometre, step by step: on the flat, up a ramp
    /// and onto the deck, jumping, crouched and prone, and off the side of a ramp (where the stage-1 client was
    /// stranded: its claim was refused and nothing put it right).
    #[test]
    fn the_prediction_is_what_the_server_does_on_the_flat_up_a_ramp_jumping_crouched_prone_and_off_a_ramps_side() {
        let level = Feast::new(false).level;
        let ramp = level.map.ramps.iter().find(|r| r.rise == kitchen_sim::map::Side::PlusZ).map(|r| r.rect).expect("the bundled map's ramp up to the deck");
        let mut routes: Vec<(&str, [f32; 3], Vec<Command>)> = vec![];
        let mut seq = 0;
        let mut run = |stick: Vec2, yaw: f32, buttons: u16, n: usize| -> Vec<Command> {
            (0..n)
                .map(|k| {
                    seq += 1;
                    walk(seq, stick, yaw, if k == 0 { buttons } else { buttons & !button::JUMP })
                })
                .collect()
        };
        routes.push(("flat", [0., FLOOR, 2.4], run(Vec2::new(0.3, -1.), 0.7, 0, 90)));
        routes.push(("jump", [0., FLOOR, 2.4], [run(Vec2::new(0., -1.), 0., button::JUMP, 40), run(Vec2::new(1., 0.), 0., button::JUMP, 40)].concat()));
        routes.push(("crouch and prone", [0., FLOOR, 2.4], [run(Vec2::new(0., -1.), 2., button::CROUCH, 40), run(Vec2::new(0., -1.), 2., button::PRONE, 40)].concat()));
        // A jump with crouch held in the air (the knees tucked up: kitchen_sim actor.rs `Actor::tuck`), landing crouched.
        routes.push(("tucked jump", [0., FLOOR, 2.4], [run(Vec2::new(0., -1.), 0., button::JUMP, 3), run(Vec2::new(0.4, -1.), 0., button::CROUCH | button::JUMP, 50)].concat()));
        // Up the ramp (facing +z, it rises 0.28 m over 0.7 m) to 0.24 m above the counter, off its side, and back
        // into its side, which is a wall from below.
        let start = [((ramp[0] + ramp[2]) / 2.) as f32, FLOOR, ramp[1] as f32 - 0.06];
        routes.push(("up a ramp and off its side", start, [run(Vec2::new(0., -1.), PI, 0, 66), run(Vec2::new(1., 0.), PI, 0, 40), run(Vec2::new(-1., 0.), PI, 0, 40)].concat()));
        for (name, at, commands) in routes {
            let (mut server, mut client, mut ghosts) = pair(Map::bundled(), at);
            let mut predictor = Predictor::default();
            let mut heights = vec![];
            for c in &commands {
                let predicted = predictor.step(&mut client, &mut ghosts, c, &ctx()).unwrap();
                serve(&mut server, 1, c, 1. / 60.);
                let truth = Vec3::from_array(server.players[0].p);
                assert!(predicted.distance(truth) < 1e-5, "{name}: command {}: predicted {predicted} but the server has {truth}", c.seq);
                heights.push(truth.y);
                // The server's word agrees: nothing to correct.
                let mine = Mine::of(server.body(1).unwrap());
                assert_eq!(predictor.reconcile(&mut client, &mut ghosts, c.seq, &mine, &ctx()), None, "{name}: {}", c.seq);
            }
            let (low, high) = heights.iter().fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
            if name.contains("jump") || name.starts_with("up") {
                assert!(high - low > 0.05, "{name} left the counter: {low}..{high}");
            }
        }
    }
    /// The server says something the client could not know (a shove): the commands since are applied again from
    /// there, the body ends where the server will have it, and a later frame agrees. A correction too far is snapped.
    #[test]
    fn a_shove_the_client_did_not_see_is_put_right_by_applying_the_commands_since_again() {
        let (mut server, mut client, mut ghosts) = pair(Map::bundled(), [0., FLOOR, 2.4]);
        let mut predictor = Predictor::default();
        let commands: Vec<Command> = (1..=30).map(|k| walk(k, Vec2::new(0., -1.), 0.3, 0)).collect();
        // The client runs ahead of the server by ten commands (the journey there and back).
        for c in &commands[..10] {
            predictor.step(&mut client, &mut ghosts, c, &ctx());
        }
        for (k, c) in commands.iter().enumerate().skip(10) {
            predictor.step(&mut client, &mut ghosts, c, &ctx());
            let applied = &commands[k - 10];
            serve(&mut server, 1, applied, 1. / 60.);
            if applied.seq == 5 {
                // Something on the server pushes the hotdog 4 cm to the side.
                let p = server.players[0].p;
                server.walk(1, Vec2::new(0.04, 0.), p[0]);
            }
            let mine = Mine::of(server.body(1).unwrap());
            let moved = predictor.reconcile(&mut client, &mut ghosts, applied.seq, &mine, &ctx());
            if applied.seq == 5 {
                let moved = moved.expect("the shove is corrected");
                assert!((moved.x - 0.04).abs() < 2e-3 && moved.length() < SNAP, "{moved}");
            } else if applied.seq > 5 {
                assert_eq!(moved, None, "after the correction the prediction agrees again ({})", applied.seq);
            }
            // The next frame may say the same (the server applied nothing new in between): nothing to correct.
            assert_eq!(predictor.reconcile(&mut client, &mut ghosts, applied.seq, &mine, &ctx()), None, "the same word twice ({})", applied.seq);
        }
        // Every command applied: the client's body is where the server's is.
        for c in &commands[20..] {
            serve(&mut server, 1, c, 1. / 60.);
        }
        let (a, b) = (ghosts.get(1).unwrap().position(), Vec3::from_array(server.players[0].p));
        assert!(a.distance(b) < 1e-4, "{a} {b}");
        assert_eq!(predictor.corrections, 1);
    }
    /// The timeline draws between frames, at a steady pace whatever the frames' arrival does, and carries a hotdog on
    /// past the newest frame for a moment only.
    #[test]
    fn the_timeline_draws_between_frames_smoothly_and_carries_on_only_a_little_past_the_newest() {
        let hz = 60.;
        let mut t = Timeline::default();
        let frame = |tick: u32| Moment { tick, players: vec![(2, Vec3::new(tick as f32 * 0.01, 0., 0.), 0., 0., Vec3::new(0.6, 0., 0.))], ..Default::default() };
        // Frames every two steps, arriving with up to 15 ms of jitter.
        let mut now = 0.;
        let mut drawn = vec![];
        let mut next = 0u32;
        for k in 0..600 {
            now = k as f64 / 240.;
            while (next as f64) / hz <= now - 0.05 - 0.015 * ((next * 7919 % 13) as f64 / 13.) {
                t.push(frame(next), now, hz as f32);
                next += 2;
            }
            t.advance(now, 1. / 240., hz as f32, 5.);
            if let Some(s) = t.player(2, hz as f32) {
                drawn.push((now, s.p.x));
            }
        }
        // After it has settled, the place moves at the hotdog's speed (0.6 m/s), never backwards, never jumping.
        let late: Vec<&(f64, f32)> = drawn.iter().filter(|(at, _)| *at > 0.8).collect();
        for pair in late.windows(2) {
            let v = (pair[1].1 - pair[0].1) as f64 / (pair[1].0 - pair[0].0);
            assert!((0.45..0.75).contains(&v), "drawn at {v} m/s at {}", pair[1].0);
        }
        // The frames stop: it is carried on for EXTRAPOLATE seconds at most.
        let last = t.player(2, hz as f32).unwrap().p.x;
        for k in 1..60 {
            t.advance(now + k as f64 / 60., 1. / 60., hz as f32, 5.);
        }
        let held = t.player(2, hz as f32).unwrap().p.x;
        assert!(held - last <= 0.6 * EXTRAPOLATE as f32 + 0.07, "carried on {} m", held - last);
        assert!((turn(3.0, -3.0, 0.5) - PI).abs() < 0.05, "the short way round");
        // One frame that came a quarter of a second early (or a clock that read late for it) is forgotten: within a
        // couple of seconds the clock is back behind the newest frame by the delay.
        let mut t = Timeline::default();
        let mut now = 0.;
        for k in 0..400u32 {
            now = k as f64 / 120.;
            if k % 4 == 0 {
                let tick = (k / 2) as u32 + 100;
                t.push(frame(tick), if k == 40 { now - 0.25 } else { now }, hz as f32);
            }
            t.advance(now, 1. / 120., hz as f32, 5.);
        }
        // (Frames of step 100 + 60 t come at t; the clock should be at 60 now + 100, less the delay.)
        let target = now * hz + 100. - 5.;
        assert!((t.clock - target).abs() < 1., "{} steps from where it should be at {now}", t.clock - target);
    }
}

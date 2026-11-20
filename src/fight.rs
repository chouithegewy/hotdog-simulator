// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Kung Fu in play: the glue between the fight as the client has it (`Game::kungfu`: offline the round's own view,
//! online the server's `{type:'kungfu'}` messages) and what the player sees and feels of it, beside the animation
//! (kungfu/), the head-up display (hud.rs) and the sounds (fu.rs).
//!
//! * **The fight's message and clock** ([`hear`], [`clock`]): online a `{type:'kungfu'}` message is `Game::kungfu`; each
//!   fighter's move is drawn at a clock that runs on smoothly between the server's frames.
//! * **Third person** ([`Camera`]): the view key (V to begin with, `Action::View`) puts the camera over the right
//!   shoulder, or back in first person (the game starts in first person). Over the shoulder this player's own body is
//!   drawn (arena.rs `update_players` offline, deathcam.rs `watch` online) and its first-person limbs are not
//!   (kungfu/view.rs); the camera looks the way the hotdog looks, is pulled in before a wall or a block behind it and
//!   kept above the counter, and is jolted by the blows as the first person is (kungfu/fx.rs). In either view the camera
//!   never ends up inside another hotdog ([`keep_out`]), and a fighter back after a KO looks into the ring.
//! * **The ring's edge** ([`Edge`]): near the edge of the ring a small red icon of a fall off a ledge comes up round the
//!   crosshair on the side the edge is, stronger the nearer it is, pulsing at the very edge: off the ring is a KO.
//! * **Prediction online** ([`Foresight`]): the client walks its own hotdog ahead of the server (arena/online.rs); in Kung
//!   Fu at the pace its fighter's move allows (`kungfu::pace`: none in a move, slow blocking) and carried by the move's
//!   lunge or dodge (`kungfu::lunge`), as the server will, so a lunge or a dodge is not pulled back when the server's word
//!   comes. The server's word on the fighter is a little old by the time it comes, so it is carried on: a move whose time
//!   is up has ended (unless something was pressed since it began), and a dodge pressed since, while free, has begun.
use crate::arena::Game;
use crate::controls::{Action, Bindings};
use bevy::prelude::*;
use kitchen_sim::kungfu::{self, FighterView, Kind, Move};
use kitchen_sim::net::{Command, button};

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Camera>()
            .init_resource::<Edge>()
            .add_systems(Update, (toggle, revive).before(crate::arena::tick))
            .add_systems(Update, (place, edge).chain().after(crate::arena::tick).before(crate::deathcam::watch))
            .add_systems(Update, keep_clear.after(crate::kungfu::view::update));
    }
}

// ----- The fight as the client has it.

/// Online: a `{type:'kungfu', ...View}` message (network.rs) is the fight as the server has it now (`Game::kungfu`, which
/// offline the round's own `Feast::kungfu_view` is: arena.rs). Returns whether it was one.
pub fn hear(g: &mut Game, message: &serde_json::Value) -> bool {
    match serde_json::from_value::<kungfu::View>(message.clone()) {
        Ok(view) if message["type"] == "kungfu" => {
            g.kungfu = Some(view);
            true
        }
        _ => false,
    }
}

/// The round's clock a fighter's move is drawn at (kungfu/sim.rs), `now` the real time (seconds). Offline the round's
/// own. Online the newest frame's run on by the real time since it came (at most two frames' worth), so that the moves
/// play smoothly between the server's frames (30 a second) and keep time with the events, which are shown as they come;
/// but a move that carries the hotdog (a dodge, a lunge) is drawn at the clock its body is drawn at, so that its legs and
/// its body go together: the other hotdogs' a little in the past with their places (arena/online.rs `Timeline`), this
/// player's own ahead of the server with its prediction (`Online::ahead`, as `Foresight` carries the body).
pub fn clock(g: &Game, f: &FighterView, now: f64) -> f32 {
    if !g.network {
        return g.elapsed;
    }
    let o = &g.online;
    let carries = f.mv().data().advance > 0.;
    if carries && f.id == g.me() {
        if let Some(ahead) = o.ahead() {
            return ahead;
        }
    }
    if carries {
        if let Some(newest) = o.timeline.moments.back() {
            return g.elapsed - ((newest.tick as f64 - o.timeline.clock) / o.hz.max(1.) as f64).max(0.) as f32;
        }
    }
    g.elapsed + ((now - o.heard).max(0.) as f32).min(2. / o.send_hz.max(1.))
}

// ----- Third person.

/// Whether this player wants the camera over its shoulder in Kung Fu (V turns it over; first person to begin with).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub third: bool,
}
/// Over the shoulder, the camera stands this far behind the eye, this far to its right and this far above it (metres;
/// the hotdog is 0.155 tall, its eye at 0.135).
pub const BACK: f32 = 0.3;
pub const SIDE: f32 = 0.095;
pub const RISE: f32 = 0.02;
/// The eye the camera turns about, above the counter where the hotdog stands.
const PIVOT: f32 = 0.125;
/// The camera keeps this far off what is behind it, and this far above the counter.
const CLEAR: f32 = 0.02;

/// Whether the camera is over the shoulder now: wanted, in a Kung Fu round, playing (not the demo) and alive (dead, the
/// death camera has it: deathcam.rs).
pub fn over_shoulder(g: &Game, camera: &Camera, demo: bool) -> bool {
    camera.third && g.kung_fu() && g.playing() && !demo && !crate::cards::at_table(g) && g.roster.iter().find(|p| p.id == g.me()).is_none_or(|p| p.alive)
}
/// `over_shoulder` for the world as it is (false while the game is out of it).
pub fn third_person(world: &World) -> bool {
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    match (world.get_resource::<Game>(), world.get_resource::<Camera>()) {
        (Some(g), Some(camera)) => over_shoulder(g, camera, demo),
        _ => false,
    }
}
/// Where the camera stands over the shoulder of a hotdog standing at `feet` and looking `rotation` (its yaw and pitch),
/// on `level`: behind, right of and above its eye, pulled in before what stands behind it and kept above the counter.
pub fn shoulder(level: &kitchen_sim::map::Level, feet: Vec3, rotation: Quat) -> Vec3 {
    let pivot = feet + Vec3::Y * PIVOT;
    let back = rotation * Vec3::new(SIDE, RISE, BACK);
    let (way, far) = (back.normalize_or_zero(), back.length());
    let room = level.cover_from(pivot, way).min(far + CLEAR) - CLEAR;
    let mut eye = pivot + way * room.clamp(0.04, far);
    let ground = level.ground(eye.x as f64, eye.z as f64) as f32;
    eye.y = eye.y.max(ground + CLEAR);
    eye
}
/// The camera jolts a blow gives (kungfu/fx.rs: one landed, one taken, the ground shaking), `now`: a small turn about
/// x, y and z, as the first person has them (kungfu/view.rs).
fn jolt(fx: &crate::kungfu::fx::Fx, now: f32) -> Vec3 {
    let one = |hit: Option<(f32, f32)>, scale: f32| {
        hit.map_or(0., |(at, strength)| {
            let tau = now - at;
            if (0. ..0.6).contains(&tau) { strength * scale * (tau / 0.025).min(1.) * (-tau / 0.08).exp() } else { 0. }
        })
    };
    let kick = one(fx.kicked, 1.) + one(fx.hurt, 1.6) + one(fx.shaken, 1.2);
    Vec3::new(-0.035 * kick, (now * 89.).sin() * kick * 0.012, (now * 117. + 1.).sin() * kick * 0.012)
}

/// Before the round's tick: the view key turns the camera over the shoulder or back (in Kung Fu, alive, with the game
/// holding the keys: the menu's gate keeps them from it while it is open).
fn toggle(world: &mut World) {
    let pressed = {
        let (Some(keys), Some(mouse)) = (world.get_resource::<ButtonInput<KeyCode>>(), world.get_resource::<ButtonInput<MouseButton>>()) else {
            return;
        };
        world.get_resource::<Bindings>().cloned().unwrap_or_default().just(Action::View, keys, mouse)
    };
    let fighting = world.get_resource::<Game>().is_some_and(|g| g.kung_fu() && g.playing() && g.roster.iter().find(|p| p.id == g.me()).is_none_or(|p| p.alive));
    if pressed && fighting {
        let mut camera = world.resource_mut::<Camera>();
        camera.third = !camera.third;
    }
}
/// Before the round's tick: a fighter back after a KO looks into the ring (the simulation brings it back at a start
/// facing the ring's middle, but the look is this player's own, which went on where it was when it died).
fn revive(world: &mut World, mut was: Local<Option<(u32, bool)>>) {
    let Some(g) = world.get_resource::<Game>() else {
        return;
    };
    let me = g.me();
    let alive = g.roster.iter().find(|p| p.id == me).map(|p| (p.alive, Vec3::from_array(p.p)));
    let back = alive.is_some_and(|(a, _)| a) && was.is_some_and(|(id, a)| id == me && !a);
    *was = alive.map(|(a, _)| (me, a));
    if !back || !g.kung_fu() {
        return;
    }
    let Some(ring) = kungfu::Ring::of(&g.feast.level) else {
        return;
    };
    let at = alive.map_or(g.position, |(_, p)| p);
    let to = ring.centre() - Vec2::new(at.x, at.z);
    if to.length() > 1e-3 {
        let mut g = world.resource_mut::<Game>();
        (g.yaw, g.pitch) = ((-to.x).atan2(-to.y), 0.);
    }
}
/// After the round's tick (which put the camera at the eye): over the shoulder, the camera put behind it.
fn place(world: &mut World) {
    if !third_person(world) {
        return;
    }
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let shake = world.get_resource::<crate::kungfu::fx::Fx>().map_or(Vec3::ZERO, |fx| jolt(fx, now));
    let g = world.resource::<Game>();
    let Some(cam) = g.camera() else {
        return;
    };
    let rotation = Quat::from_rotation_y(g.yaw) * Quat::from_rotation_x(g.pitch);
    let eye = shoulder(&g.feast.level, g.position, rotation);
    if let Some(mut tr) = world.get_mut::<Transform>(cam) {
        *tr = Transform::from_translation(eye).with_rotation(rotation * Quat::from_rotation_x(shake.x) * Quat::from_rotation_y(shake.y) * Quat::from_rotation_z(shake.z));
    }
}

/// How near another hotdog's middle (flat) the camera may come: the sausage's and the buns' half width and a little.
pub const KEEP_OUT: f32 = 0.05;
/// `eye` put out of any of `bodies` (each a hotdog's feet, 0.19 tall), sideways, the shortest way (or `back`, flat,
/// from dead centre).
pub fn keep_out(eye: Vec3, bodies: &[Vec3], back: Vec3) -> Vec3 {
    let mut eye = eye;
    for b in bodies {
        if eye.y < b.y - 0.01 || eye.y > b.y + 0.19 {
            continue;
        }
        let d = Vec2::new(eye.x - b.x, eye.z - b.z);
        let r = d.length();
        if r < KEEP_OUT {
            let way = if r > 1e-4 { d / r } else { Vec2::new(back.x, back.z).normalize_or(Vec2::X) };
            (eye.x, eye.z) = (b.x + way.x * KEEP_OUT, b.z + way.y * KEEP_OUT);
        }
    }
    eye
}
/// Last of the camera's moves in a fight (after the first person's nudges, kungfu/view.rs: a lunge or a lean can take
/// the eye into the hotdog it strikes; over the shoulder, one standing right behind): the camera is put out of every
/// other living hotdog, so the picture is never the inside of a bun.
fn keep_clear(world: &mut World) {
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    let Some(g) = world.get_resource::<Game>() else {
        return;
    };
    let me = g.me();
    if !g.kung_fu() || !g.playing() || demo || g.roster.iter().find(|p| p.id == me).is_some_and(|p| !p.alive) {
        return;
    }
    let Some(cam) = g.camera() else {
        return;
    };
    let bodies: Vec<Vec3> = g.roster.iter().filter(|p| p.alive && p.id != me).map(|p| Vec3::from_array(p.p)).collect();
    if let Some(mut tr) = world.get_mut::<Transform>(cam) {
        let back = tr.rotation * Vec3::Z;
        let out = keep_out(tr.translation, &bodies, back);
        if out != tr.translation {
            tr.translation = out;
        }
    }
}

// ----- The ring's edge.

/// Nearer the ring's edge than this (metres, from the hotdog's middle) the warning shows; at EDGE_NEAR it is whole.
pub const EDGE_WARN: f32 = 0.11;
pub const EDGE_NEAR: f32 = 0.035;
/// The warning's icon (pixels high at a 720-pixel window) and how far from the middle of the screen it stands.
const EDGE_ICON: f32 = 34.;
const EDGE_OUT: f32 = 62.;
const EDGE_RGB: [f32; 3] = [1., 0.26, 0.2];

/// How far a hotdog at `p` (flat: x, z) is from the edge of `ring`, and the way out to it (flat, a unit vector).
pub fn to_edge(ring: &kungfu::Ring, p: Vec2) -> (f32, Vec2) {
    let [x0, z0, x1, z1] = ring.rect.map(|v| v as f32);
    let centre = Vec2::new((x0 + x1) / 2., (z0 + z1) / 2.);
    let half = Vec2::new((x1 - x0) / 2., (z1 - z0) / 2.).max(Vec2::splat(1e-3));
    let d = p - centre;
    if ring.round {
        // Along the line out from the middle to the ellipse; the way out is the ellipse's normal there.
        let q = d / half;
        let r = q.length();
        if r < 1e-4 {
            return (half.min_element(), Vec2::X);
        }
        let far = d.length() * (1. / r - 1.);
        let out = (d / (half * half)).normalize_or(Vec2::X);
        (far, out)
    } else {
        let sides = [(d.x + half.x, Vec2::NEG_X), (half.x - d.x, Vec2::X), (d.y + half.y, Vec2::NEG_Y), (half.y - d.y, Vec2::Y)];
        sides.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap()
    }
}
/// How strongly the warning shows `far` metres from the edge: nothing beyond EDGE_WARN, whole within EDGE_NEAR.
pub fn edge_strength(far: f32) -> f32 {
    let t = ((EDGE_WARN - far) / (EDGE_WARN - EDGE_NEAR)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// Where the warning stands round the crosshair for an edge the way `out` (flat, world) of a hotdog looking `yaw`:
/// the angle from straight up the screen, clockwise (ahead is up, to the right is right, behind is down).
pub fn edge_angle(out: Vec2, yaw: f32) -> f32 {
    let ahead = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(yaw.cos(), -yaw.sin());
    out.dot(right).atan2(out.dot(ahead))
}

/// The warning's node, made the first time it is wanted.
#[derive(Resource, Default)]
pub struct Edge {
    node: Option<Entity>,
    /// How strongly it shows now (0 hidden), for the tests.
    pub shown: f32,
    /// The angle it stands at round the crosshair (see `edge_angle`).
    pub angle: f32,
}
/// Every frame after the round: the ring's edge warning, for this player alive in a Kung Fu round.
fn edge(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    let (strength, angle) = match world.get_resource::<Game>() {
        Some(g) if g.kung_fu() && g.playing() && !demo && g.roster.iter().find(|p| p.id == g.me()).is_some_and(|p| p.alive) => {
            match kungfu::Ring::of(&g.feast.level) {
                Some(ring) => {
                    let (far, out) = to_edge(&ring, Vec2::new(g.position.x, g.position.z));
                    (edge_strength(far), edge_angle(out, g.yaw))
                }
                None => (0., 0.),
            }
        }
        _ => (0., 0.),
    };
    let (s, sf) = world.query::<&Window>().iter(world).next().map_or((1., 1.), |w| ((w.height() / 720.).clamp(0.8, 4.), w.scale_factor()));
    let mut state = world.remove_resource::<Edge>().unwrap_or_default();
    (state.shown, state.angle) = (strength, angle);
    if state.node.is_none_or(|e| world.get_entity(e).is_err()) {
        if strength <= 0. {
            world.insert_resource(state);
            return;
        }
        let height = (EDGE_ICON * s * sf).round().max(8.) as u32;
        if !world.contains_resource::<crate::icons::Icons>() {
            world.init_resource::<crate::icons::Icons>();
        }
        let handle = world.resource_scope(|world, mut icons: Mut<crate::icons::Icons>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            icons.get(&mut images, crate::icons::Icon::Fall, height)
        });
        let node = world
            .spawn((
                Name::new("Kung Fu: the ring's edge"),
                ImageNode::new(handle),
                Node { position_type: PositionType::Absolute, left: Val::Percent(50.), top: Val::Percent(50.), ..default() },
                GlobalZIndex(5),
                Visibility::Hidden,
            ))
            .id();
        state.node = Some(node);
    }
    let node = state.node.unwrap();
    let (w, h) = crate::icons::size(crate::icons::Icon::Fall, (EDGE_ICON * s * sf).round().max(8.) as u32);
    let (w, h) = (w as f32 / sf, h as f32 / sf);
    // (At the very edge it pulses.)
    let pulse = if strength > 0.85 { 0.7 + 0.3 * (now * 14.).sin().abs() } else { 1. };
    let alpha = strength * pulse;
    let (dx, dy) = (angle.sin() * EDGE_OUT * s, -angle.cos() * EDGE_OUT * s);
    if let Some(mut n) = world.get_mut::<Node>(node) {
        (n.width, n.height) = (Val::Px(w), Val::Px(h));
        n.margin = UiRect { left: Val::Px(dx - w / 2.), top: Val::Px(dy - h / 2.), ..default() };
    }
    if let Some(mut image) = world.get_mut::<ImageNode>(node) {
        image.color = Color::srgba(EDGE_RGB[0], EDGE_RGB[1], EDGE_RGB[2], alpha);
        // The icon's figure falls off to the right of its ledge: turned round for an edge on the left.
        image.flip_x = angle < 0.;
    }
    let want = if alpha > 0.01 { Visibility::Visible } else { Visibility::Hidden };
    if world.get::<Visibility>(node).is_some_and(|v| *v != want) {
        world.entity_mut(node).insert(want);
    }
    world.insert_resource(state);
}

// ----- Prediction online.

/// Kung Fu online: what the prediction (arena/online.rs) needs to walk this player's hotdog as the server will.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foresight {
    /// This player's fighter as the server last told it.
    pub fighter: FighterView,
    /// The server's round clock just after the step that applied command `.0` (a frame's `ack` and `elapsed`): the
    /// server applies one command a step, so each command's step is known from it.
    pub clock: (u32, f32),
}
/// What a command's step is, foreseen: the share of the stance's walk it walks at, whether a jump may start, and how
/// far the move carries the hotdog (flat, world: x, z).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Step {
    pub pace: f32,
    pub free: bool,
    pub lunge: Vec2,
}
impl Foresight {
    /// The server's round clock just after the step that applies command `seq`.
    pub fn at(&self, seq: u32, dt: f32) -> f32 {
        self.clock.1 + (seq as i64 - self.clock.0 as i64) as f32 * dt
    }
    /// The fighter as the server will have it at `now`, the commands before `seq` (`before`, oldest first) carried on
    /// from what it last told: a dodge pressed while free has begun, a move whose time is up has ended (unless something
    /// else was pressed since it began: the server may have started that instead, and its word will say).
    pub fn fighter(&self, seq: u32, before: &[Command], now: f32, dt: f32) -> FighterView {
        let mut f = self.fighter.clone();
        let mut busy = false;
        let mut prev = 0u16;
        for (k, p) in before.iter().enumerate().filter(|(_, p)| p.seq < seq) {
            let held = if k == 0 { p.buttons } else { prev };
            prev = p.buttons;
            // (The step after the command is when a press of it starts a move; the server's word may have it already.)
            let at = self.at(p.seq, dt);
            if at <= f.started + dt * 0.5 {
                continue;
            }
            let rose = |bit: u16| p.has(bit) && held & bit == 0;
            let attack = rose(button::FIRE) || rose(button::SCOPE) || rose(button::PRONE) || p.has(button::MELEE) || p.has(button::THROW) || p.has(button::EAT);
            if attack {
                busy = true;
            } else if p.has(button::STIM) {
                let was = settle(&f, at, busy);
                let ready = f.mv().data().kind != Kind::Dodge || at >= f.started + f.len + kungfu::DODGE_WAIT;
                if kungfu::free(&was, at) && ready {
                    let w = p.stick();
                    let mv = if w.y > 0.3 || w.length() < 0.3 {
                        Move::BackHop
                    } else if w.x < 0. {
                        Move::DodgeLeft
                    } else {
                        Move::DodgeRight
                    };
                    f = FighterView { mv: mv as u8, started: at, len: mv.data().total(), scale: 1., connected: false, block: kungfu::NO_BLOCK, ..was };
                    busy = false;
                }
            }
        }
        settle(&f, now, busy)
    }
    /// Command `c`'s step foreseen, the commands before it being `before` (oldest first; any at or after it are passed
    /// over): the server walks it at its fighter's pace as the command is applied (`Feast::kungfu_command`), and the
    /// step after carries it by the move's lunge or dodge along the way the command looks (kungfu.rs `kungfu_run`).
    pub fn step(&self, c: &Command, before: &[Command], dt: f32) -> Step {
        let after = self.at(c.seq, dt);
        let now = after - dt;
        let f = self.fighter(c.seq, before, now, dt);
        let free = kungfu::free(&f, now);
        let pace = if free { kungfu::pace(&FighterView { mv: if f.block != kungfu::NO_BLOCK { f.mv } else { Move::Idle as u8 }, ..f.clone() }) } else { 0. };
        let metres = kungfu::lunge(&f, after, dt);
        Step { pace, free, lunge: carry_way(f.mv(), c.yaw()) * metres }
    }
}
/// The way a move carries a fighter looking `yaw` (flat, world: x, z), as the simulation has it (kungfu.rs
/// `kungfu_run`): ahead, a side step to its side, the back hop back. (The simulation's side steps were found mirrored,
/// DodgeLeft carrying the fighter to its right: reported to KF1 in framework-requests.md. This follows the simulation
/// whichever way it goes, and `the_foresight_matches_the_simulation` keeps the two in step.)
pub fn carry_way(mv: Move, yaw: f32) -> Vec2 {
    let ahead = Vec2::new(-yaw.sin(), -yaw.cos());
    match mv {
        Move::DodgeLeft => Vec2::new(ahead.y, -ahead.x), // KF1: the sim's side steps go to the fighter's own left and right
        Move::DodgeRight => Vec2::new(-ahead.y, ahead.x),
        Move::BackHop => -ahead,
        _ => ahead,
    }
}
/// A fighter as it will be at `now`: a move whose time is up ended into the stance, or a knockdown into getting up
/// (not when something was pressed since the move began: `busy`); a block's stun over, the block still held.
fn settle(f: &FighterView, now: f32, busy: bool) -> FighterView {
    let mut f = f.clone();
    for _ in 0..2 {
        if f.len <= 0. || f.t(now) < f.len {
            break;
        }
        let end = f.started + f.len;
        match f.mv() {
            Move::BlockHigh | Move::BlockLow => f.len = 0.,
            Move::KnockDown => f = FighterView { mv: Move::GetUp as u8, started: end, len: Move::GetUp.data().total(), scale: 1., down: false, connected: false, ..f },
            _ if busy => break,
            _ => f = FighterView { mv: Move::Idle as u8, started: end, len: 0., scale: 1., connected: false, stunned: false, down: false, block: kungfu::NO_BLOCK, ..f },
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::kungfu::{Ring, Style};

    const DT: f32 = 1. / 60.;
    fn command(seq: u32, stick: Vec2, buttons: u16) -> Command {
        Command::new(seq, stick, 0., 0., buttons, 0, None)
    }
    fn fighter(mv: Move, started: f32) -> FighterView {
        FighterView { id: 1, style: Style::Crane as u8, mv: mv as u8, started, len: if mv.data().kind == Kind::Stance { 0. } else { mv.data().total() }, scale: 1., guard: 100., ..default() }
    }

    /// Free, the stance's pace and nothing carried; in a strike nothing, and once its time is up the stance again
    /// (unless something was pressed since); a dodge pressed while free begins at the step after its command and carries
    /// the hotdog sideways by the table's distance over its window, as `kungfu::lunge` has the server do.
    #[test]
    fn the_prediction_walks_and_lunges_as_the_server_will() {
        let style = kungfu::STYLE_PACE[Style::Crane as usize];
        let sight = Foresight { fighter: fighter(Move::Idle, 0.), clock: (100, 10.) };
        let s = sight.step(&command(101, Vec2::NEG_Y, 0), &[], DT);
        assert!(s.free && (s.pace - kungfu::STANCE_PACE * style).abs() < 1e-6 && s.lunge == Vec2::ZERO, "{s:?}");
        // A heavy strike begun at 10 s: no walking in it; ended, the stance; pressed for again meanwhile, not foreseen.
        let heavy = Move::CraneHeavy1;
        let sight = Foresight { fighter: fighter(heavy, 10.), clock: (100, 10.) };
        assert_eq!(sight.step(&command(110, Vec2::ZERO, 0), &[], DT).pace, 0.);
        let late = 100 + (heavy.data().total() / DT).ceil() as u32 + 2;
        assert!(sight.step(&command(late, Vec2::ZERO, 0), &[], DT).pace > 0.5 * kungfu::STANCE_PACE);
        let mashed = [command(105, Vec2::ZERO, 0), command(106, Vec2::ZERO, button::FIRE)];
        assert_eq!(sight.step(&command(late, Vec2::ZERO, 0), &mashed, DT).pace, 0.);
        // A dodge to the left (the stick left) pressed in command 101: from the step after it, sideways; in all, the
        // table's distance (the yaw is 0: ahead is -z, the left is -x).
        let sight = Foresight { fighter: fighter(Move::Idle, 0.), clock: (100, 10.) };
        let dodge = command(101, Vec2::NEG_X, button::STIM);
        let mut before = vec![dodge];
        let mut carried = Vec2::ZERO;
        for seq in 102..160 {
            let c = command(seq, Vec2::ZERO, 0);
            let s = sight.step(&c, &before, DT);
            assert_eq!(s.pace, if seq < 101 + (Move::DodgeLeft.data().total() / DT) as u32 { 0. } else { s.pace });
            carried += s.lunge;
            before.push(c);
        }
        // (Which side is the simulation's: `the_foresight_matches_the_simulation`.)
        let d = Move::DodgeLeft.data().advance;
        assert!((carried.length() - d).abs() < 0.01 && carried.y.abs() < 1e-4, "{carried:?}");
        // The server's word on that dodge (begun the step after 101) makes the same foresight.
        let told = Foresight { fighter: FighterView { started: sight.at(101, DT), ..fighter(Move::DodgeLeft, 0.) }, clock: (100, 10.) };
        let a = sight.step(&command(110, Vec2::ZERO, 0), &before[..9], DT);
        let b = told.step(&command(110, Vec2::ZERO, 0), &before[..9], DT);
        assert!((a.lunge - b.lunge).length() < 1e-6 && a.pace == b.pace, "{a:?} {b:?}");
        // Blocking: the block's pace, none while a blocked blow's stun lasts.
        let block = FighterView { block: kungfu::BLOCK_HIGH, len: 0.3, ..fighter(Move::BlockHigh, 10.) };
        let sight = Foresight { fighter: block, clock: (100, 10.) };
        assert_eq!(sight.step(&command(102, Vec2::ZERO, button::GRAB), &[], DT).pace, 0.);
        assert!((sight.step(&command(130, Vec2::ZERO, button::GRAB), &[], DT).pace - kungfu::BLOCK_PACE * style).abs() < 1e-6);
    }

    /// The camera is put out of another hotdog, sideways, and left alone beside, above or below one.
    #[test]
    fn the_camera_is_kept_out_of_other_hotdogs() {
        let body = Vec3::new(0., 0.93, 2.8);
        let inside = body + Vec3::new(0.02, 0.12, 0.);
        assert!((keep_out(inside, &[body], Vec3::Z) - (body + Vec3::new(KEEP_OUT, 0.12, 0.))).length() < 1e-5);
        let centre = body + Vec3::Y * 0.1;
        assert!((keep_out(centre, &[body], Vec3::Z) - (centre + Vec3::Z * KEEP_OUT)).length() < 1e-5, "from dead centre, back");
        for clear in [body + Vec3::new(0.06, 0.1, 0.), body + Vec3::new(0., 0.25, 0.), body + Vec3::new(0., -0.05, 0.)] {
            assert_eq!(keep_out(clear, &[body], Vec3::Z), clear);
        }
    }

    /// The ring's edge: how far and which way, round or square; the warning's strength; where it stands on the screen.
    #[test]
    fn the_rings_edge_is_measured_and_shown_on_its_side() {
        let ring = Ring { rect: [-0.5, 2.3, 0.5, 3.3], top: 0.93, round: true };
        let (far, out) = to_edge(&ring, Vec2::new(0.3, 2.8));
        assert!((far - 0.2).abs() < 1e-4 && (out - Vec2::X).length() < 1e-4, "{far} {out:?}");
        let (far, out) = to_edge(&ring, Vec2::new(0., 2.36));
        assert!((far - 0.06).abs() < 1e-4 && (out - Vec2::NEG_Y).length() < 1e-4, "{far} {out:?}");
        let square = Ring { round: false, ..ring };
        let (far, out) = to_edge(&square, Vec2::new(0.45, 2.5));
        assert!((far - 0.05).abs() < 1e-4 && out == Vec2::X);
        assert_eq!(edge_strength(0.3), 0.);
        assert_eq!(edge_strength(0.), 1.);
        assert!(edge_strength(0.07) > 0. && edge_strength(0.07) < 1.);
        // Looking -z (yaw 0): an edge ahead is up the screen, to the right (+x) is right, behind is down.
        assert!(edge_angle(Vec2::NEG_Y, 0.).abs() < 1e-5);
        assert!((edge_angle(Vec2::X, 0.) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((edge_angle(Vec2::Y, 0.).abs() - std::f32::consts::PI).abs() < 1e-5);
    }

    // ----- In the game (arena.rs `tick`, with this module's systems).

    use crate::arena::{apply_frame, test_app, test_frame};
    use bevy::input::mouse::AccumulatedMouseScroll;
    use kitchen_sim::feast::Feast;
    use kitchen_sim::kungfu::Slot;
    use kitchen_sim::map::Map;

    fn dojo_map() -> Map {
        Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/kung-fu-dojo.json")).unwrap()).unwrap()
    }
    /// Single player on the dojo by `rules` (the page's words), the round's tick and this module's systems added, a few
    /// frames in.
    fn dojo(rules: &str) -> App {
        let mut app = test_app(rules);
        app.add_plugins(Plugin).add_systems(Update, crate::arena::tick);
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast = Feast::with_map(dojo_map(), false);
            g.begin();
        }
        for _ in 0..3 {
            test_frame(&mut app, 0.05);
        }
        assert!(app.world().resource::<Game>().kung_fu(), "the dojo is Kung Fu's");
        app
    }
    fn press(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
    }
    fn release(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
    }
    fn mine(app: &App) -> FighterView {
        app.world().resource::<Game>().fighter(1).cloned().expect("the client has the fight")
    }

    /// Single player: the left button is the light strike (the style's first), not a shot; the client's view of the
    /// fight is the round's own, every frame.
    #[test]
    fn offline_the_left_button_strikes_light_and_the_view_is_the_rounds() {
        let mut app = dojo("sandbox botmode=idle");
        {
            let g = app.world().resource::<Game>();
            assert_eq!(g.kungfu, g.feast.kungfu_view(), "the round's own view");
            assert_eq!(mine(&app).mv(), Move::Idle);
        }
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        test_frame(&mut app, 1. / 60.);
        let f = mine(&app);
        assert_eq!(f.mv(), f.style().move_in(Slot::Light1), "{:?}", f.mv());
        let g = app.world().resource::<Game>();
        assert!(!g.pending_shot && g.feast.shots.iter().all(|s| s.owner != 1), "no shot");
        assert_eq!(g.feast.player(1).unwrap().ammo, [0; kitchen_sim::feast::GUNS]);
        assert_eq!(g.kungfu, g.feast.kungfu_view(), "the view follows the round");
        // No gun in view, none even made: hands and feet only.
        let guns = |app: &mut App| app.world_mut().query_filtered::<&Visibility, With<crate::viewmodel::ViewModel>>().iter(app.world()).count();
        assert_eq!(guns(&mut app), 0, "no gun viewmodel in Kung Fu");
        // (A siege has its guns: the check above is not empty for want of a camera.)
        let mut siege = test_app("");
        siege.add_systems(Update, crate::arena::tick);
        for _ in 0..3 {
            test_frame(&mut siege, 0.05);
        }
        assert!(guns(&mut siege) > 0);
    }

    /// Online: the server's `{type:'kungfu'}` message is the client's fight; the left button goes as the light strike's
    /// FIRE (no shot), the block's GRAB is in every command while Q is held and none after, and a style's key asks for it
    /// as the command's weapon (style + 1), as does the wheel.
    #[test]
    fn online_the_fight_is_the_servers_word_and_the_keys_go_as_the_interface_says() {
        let mut app = test_app("mode=kungfu");
        app.add_plugins(Plugin).add_systems(Update, crate::arena::tick);
        app.world_mut().insert_resource(crate::lobby::Lobby::default());
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            (g.network, g.network_id) = (true, 1);
        }
        let mut feast = Feast::with_map(Map::bundled(), false);
        feast.set_rules(kitchen_sim::rules::Rules::parse("mode=kungfu"));
        feast.tick(0.5);
        let players: Vec<kitchen_sim::net::Seat> = feast.players.iter().map(|p| kitchen_sim::net::Seat { player: p.clone(), human: p.id == 1, ..Default::default() }).collect();
        let frame = |tick: u32| kitchen_sim::net::Frame { tick, round: 1, rules: "mode=kungfu".into(), stage: "FreeForAll".into(), players: players.clone(), elapsed: feast.elapsed, ..Default::default() };
        apply_frame(app.world_mut(), &frame(1));
        // The server's word on the fight.
        let view = feast.kungfu_view().expect("a Kung Fu round");
        let mut message = serde_json::to_value(&view).unwrap();
        message["type"] = "kungfu".into();
        assert!(!hear(&mut app.world_mut().resource_mut::<Game>(), &serde_json::json!({"type": "cards"})));
        assert!(hear(&mut app.world_mut().resource_mut::<Game>(), &message));
        assert_eq!(app.world().resource::<Game>().kungfu.as_ref(), Some(&view));
        test_frame(&mut app, 0.05);
        let style = mine(&app).style;
        let commands = |app: &mut App| std::mem::take(&mut app.world_mut().resource_mut::<Game>().online.unsent);
        commands(&mut app);
        // The left button: FIRE in the commands, and no shot.
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        for k in 2..5 {
            test_frame(&mut app, 0.05);
            apply_frame(app.world_mut(), &frame(k));
        }
        let sent = commands(&mut app);
        assert!(!sent.is_empty() && sent.iter().all(|c| c.has(button::FIRE) && c.weapon == style + 1), "{sent:?}");
        assert!(!app.world().resource::<Game>().pending_shot);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        test_frame(&mut app, 0.05);
        commands(&mut app);
        // Q held over several frames: GRAB in every command; let go, in none.
        press(&mut app, KeyCode::KeyQ);
        for _ in 0..4 {
            test_frame(&mut app, 0.05);
        }
        let sent = commands(&mut app);
        assert!(sent.len() >= 8 && sent.iter().all(|c| c.has(button::GRAB) && !c.has(button::FIRE)), "{sent:?}");
        release(&mut app, KeyCode::KeyQ);
        test_frame(&mut app, 0.05);
        commands(&mut app);
        test_frame(&mut app, 0.05);
        assert!(commands(&mut app).iter().all(|c| !c.has(button::GRAB)));
        // 3: the Snake (style 2), sent as weapon 3; the wheel down, the next (the Mantis, 4).
        press(&mut app, KeyCode::Digit3);
        test_frame(&mut app, 0.05);
        assert!(commands(&mut app).iter().all(|c| c.weapon == 3));
        *app.world_mut().resource_mut::<AccumulatedMouseScroll>() = AccumulatedMouseScroll { unit: bevy::input::mouse::MouseScrollUnit::Line, delta: Vec2::new(0., -1.) };
        test_frame(&mut app, 0.2);
        assert!(commands(&mut app).iter().all(|c| c.weapon == 4));
    }

    /// V turns the camera over the shoulder in Kung Fu (this player's body seen, the camera behind it) and back into
    /// first person (the body not drawn, the camera at the eye); outside Kung Fu it does nothing.
    #[test]
    fn the_view_key_turns_the_camera_over_the_shoulder_and_back() {
        let mut app = dojo("sandbox botmode=idle");
        let camera = |app: &mut App| {
            let cam = app.world().resource::<Game>().camera().unwrap();
            *app.world().get::<Transform>(cam).unwrap()
        };
        let body = |app: &mut App| {
            let mut q = app.world_mut().query::<(&crate::player_model::Gait, &Visibility)>();
            q.iter(app.world()).find(|(g, _)| g.id() == 1).map(|(_, v)| *v).expect("this player's body")
        };
        let eye = |app: &App| app.world().resource::<Game>().position + Vec3::Y * 0.135;
        test_frame(&mut app, 0.05);
        assert!(!app.world().resource::<Camera>().third, "first person to begin with");
        assert!(camera(&mut app).translation.distance(eye(&app)) < 0.03);
        assert_eq!(body(&mut app), Visibility::Hidden);
        press(&mut app, KeyCode::KeyV);
        test_frame(&mut app, 0.05);
        test_frame(&mut app, 0.05);
        assert!(app.world().resource::<Camera>().third);
        let g = app.world().resource::<Game>();
        let want = shoulder(&g.feast.level, g.position, Quat::from_rotation_y(g.yaw) * Quat::from_rotation_x(g.pitch));
        let at = camera(&mut app);
        assert!(at.translation.distance(want) < 1e-3 && at.translation.distance(eye(&app)) > 0.2, "{at:?} {want:?}");
        assert_eq!(body(&mut app), Visibility::Visible, "its own body is seen");
        release(&mut app, KeyCode::KeyV);
        test_frame(&mut app, 0.05);
        press(&mut app, KeyCode::KeyV);
        test_frame(&mut app, 0.05);
        assert!(!app.world().resource::<Camera>().third);
        assert!(camera(&mut app).translation.distance(eye(&app)) < 0.03);
        assert_eq!(body(&mut app), Visibility::Hidden);
        // A siege: V is nothing.
        let mut siege = test_app("");
        siege.add_plugins(Plugin).add_systems(Update, crate::arena::tick);
        for _ in 0..3 {
            test_frame(&mut siege, 0.05);
        }
        press(&mut siege, KeyCode::KeyV);
        test_frame(&mut siege, 0.05);
        assert!(!siege.world().resource::<Camera>().third);
    }

    /// The foresight walks and carries the hotdog as the simulation itself does, command by command: a side step each
    /// way and the back hop foreseen from the stance (the server's word not yet come), a lunging heavy from the server's
    /// word on it, and the walk at a style's pace, also on after the heavy is over.
    #[test]
    fn the_foresight_matches_the_simulation() {
        use kitchen_sim::kungfu::Style;
        // A round on the dojo, the bots idle, this player alone in the middle of the ring, looking -z, in `style`.
        let start = |style: Style| {
            let mut feast = Feast::with_map(dojo_map(), false);
            feast.set_rules(kitchen_sim::rules::Rules::parse("sandbox botmode=idle"));
            let centre = Ring::of(&feast.level).unwrap().centre();
            let y = feast.level.ground(centre.x as f64, centre.y as f64);
            if let Some(b) = feast.bodies.get_mut(1) {
                (b.p, b.v) = ([centre.x as f64, y, centre.y as f64], [0.; 3]);
            }
            let mut seq = 0;
            for _ in 0..40 {
                seq += 1;
                feast.kungfu_command(1, &Command::new(seq, Vec2::ZERO, 0., 0., 0, style as u8 + 1, None), DT);
                feast.tick(DT);
            }
            assert_eq!(feast.kungfu_view().unwrap().fighter(1).unwrap().style(), style);
            (feast, seq)
        };
        let at = |feast: &Feast| {
            let p = feast.player(1).unwrap().p;
            Vec2::new(p[0], p[2])
        };
        // Plays `script` (the stick and the buttons, a command a step); from the `told`th on, the foresight has the
        // server's word as it stood then. Returns how far the simulation took the hotdog from then, and the foresight.
        let play = |style: Style, script: &[(Vec2, u16)], told: usize| {
            let (mut feast, mut seq) = start(style);
            let (mut sight, mut sent, mut from, mut foreseen) = (None, vec![], Vec2::ZERO, Vec2::ZERO);
            for (k, (stick, buttons)) in script.iter().enumerate() {
                if k == told {
                    sight = Some(Foresight { fighter: feast.kungfu_view().unwrap().fighter(1).unwrap().clone(), clock: (seq, feast.elapsed) });
                    from = at(&feast);
                }
                seq += 1;
                let c = Command::new(seq, *stick, 0., 0., *buttons, style as u8 + 1, None);
                if let Some(sight) = &sight {
                    let s = sight.step(&c, &sent, DT);
                    foreseen += c.stride(feast.tune.pace(false, false, false, 1.) * s.pace, DT) + s.lunge;
                    sent.push(c);
                }
                feast.kungfu_command(1, &c, DT);
                feast.tick(DT);
            }
            (at(&feast) - from, foreseen)
        };
        let idle = |n: usize, stick: Vec2| vec![(stick, 0u16); n];
        let cases: Vec<(&str, Style, Vec<(Vec2, u16)>, usize)> = vec![
            ("side step left", Style::Crane, [vec![(Vec2::NEG_X, button::STIM)], idle(40, Vec2::ZERO)].concat(), 0),
            ("side step right", Style::Crane, [vec![(Vec2::X, button::STIM)], idle(40, Vec2::ZERO)].concat(), 0),
            ("back hop", Style::Crane, [vec![(Vec2::ZERO, button::STIM)], idle(40, Vec2::ZERO)].concat(), 0),
            ("the Monkey's side step", Style::Monkey, [vec![(Vec2::X, button::STIM)], idle(40, Vec2::ZERO)].concat(), 0),
            ("a lunging heavy", Style::Monkey, [vec![(Vec2::ZERO, button::SCOPE)], idle(50, Vec2::ZERO)].concat(), 3),
            ("the Toad's walk", Style::Toad, idle(30, Vec2::NEG_Y), 0),
            ("walking on after a heavy", Style::Monkey, [vec![(Vec2::ZERO, button::SCOPE)], idle(80, Vec2::NEG_Y)].concat(), 3),
        ];
        for (name, style, script, told) in cases {
            let (moved, foreseen) = play(style, &script, told);
            assert!(moved.length() > 0.05, "{name}: the simulation moved it {moved:?}");
            assert!((moved - foreseen).length() < 0.002, "{name}: the simulation moved it {moved:?}, the foresight {foreseen:?}");
        }
    }

    /// Online a move is drawn at the newest frame's clock run on smoothly since it came (two frames' worth at most), and
    /// a move that carries the hotdog at its body's clock: another's a little in the past with the timeline, this
    /// player's own ahead with its prediction. Offline, the round's own clock.
    #[test]
    fn online_moves_are_drawn_smoothly_and_a_carrying_one_with_its_body() {
        let mut app = test_app("");
        let mut g = app.world_mut().resource_mut::<Game>();
        (g.network, g.network_id, g.elapsed) = (true, 1, 10.);
        (g.online.heard, g.online.clock, g.online.seq) = (100., (50, 10.), 55);
        let mut newest = crate::arena::online::Moment::default();
        newest.tick = 600;
        g.online.timeline.moments.push_back(newest);
        g.online.timeline.clock = 595.;
        let idle = FighterView { id: 2, mv: Move::Idle as u8, ..default() };
        assert_eq!(clock(&g, &idle, 100.), 10.);
        assert!((clock(&g, &idle, 100.01) - 10.01).abs() < 1e-4);
        assert!((clock(&g, &idle, 101.) - (10. + 2. / 30.)).abs() < 1e-4, "no further than two frames");
        let dodge = FighterView { id: 2, mv: Move::DodgeLeft as u8, ..default() };
        assert!((clock(&g, &dodge, 100.) - (10. - 5. / 60.)).abs() < 1e-4, "five steps behind, with its place");
        let mine = FighterView { id: 1, ..dodge.clone() };
        assert!((clock(&g, &mine, 100.) - (10. + 4. / 60.)).abs() < 1e-4, "ahead with the prediction");
        let mine_idle = FighterView { id: 1, ..idle };
        assert_eq!(clock(&g, &mine_idle, 100.), 10.);
        g.network = false;
        assert_eq!(clock(&g, &dodge, 100.), 10.);
    }

    /// Near the ring's edge the warning shows, on the side the edge is; in the middle it does not.
    #[test]
    fn near_the_rings_edge_the_warning_shows_on_its_side() {
        let mut app = dojo("sandbox botmode=idle");
        let ring = Ring::of(&app.world().resource::<Game>().feast.level).expect("the dojo's ring");
        let centre = ring.centre();
        let put = |app: &mut App, x: f32, z: f32| {
            let mut g = app.world_mut().resource_mut::<Game>();
            let y = g.feast.level.ground(x as f64, z as f64);
            if let Some(b) = g.feast.bodies.get_mut(1) {
                b.p = [x as f64, y, z as f64];
            }
        };
        put(&mut app, centre.x, centre.y);
        test_frame(&mut app, 0.05);
        assert_eq!(app.world().resource::<Edge>().shown, 0.);
        // 4 cm inside the edge, to the right (+x): the warning shows, to the right of the crosshair looking -z.
        put(&mut app, centre.x + ring.radius() - 0.04, centre.y);
        app.world_mut().resource_mut::<Game>().yaw = 0.;
        test_frame(&mut app, 0.05);
        let edge = app.world().resource::<Edge>();
        assert!(edge.shown > 0.5, "{}", edge.shown);
        assert!((edge.angle - std::f32::consts::FRAC_PI_2).abs() < 0.2, "{}", edge.angle);
        let node = edge.node.unwrap();
        assert_eq!(app.world().get::<Visibility>(node), Some(&Visibility::Visible));
    }

    /// Rung out while looking away from the ring, this player comes back looking into it.
    #[test]
    fn back_after_a_ko_the_fighter_looks_into_the_ring() {
        let mut app = dojo("sandbox botmode=idle");
        let ring = Ring::of(&app.world().resource::<Game>().feast.level).expect("the dojo's ring");
        let c = ring.centre();
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            let (x, z) = (c.x + ring.radius() + 0.15, c.y);
            let y = g.feast.level.ground(x as f64, z as f64);
            if let Some(b) = g.feast.bodies.get_mut(1) {
                b.p = [x as f64, y, z as f64];
            }
            g.yaw = -std::f32::consts::FRAC_PI_2;
        }
        let alive = |app: &App| app.world().resource::<Game>().roster.iter().find(|p| p.id == 1).is_some_and(|p| p.alive);
        for _ in 0..20 {
            test_frame(&mut app, 0.05);
            if !alive(&app) {
                break;
            }
        }
        assert!(!alive(&app), "rung out");
        for _ in 0..300 {
            test_frame(&mut app, 0.05);
            if alive(&app) {
                break;
            }
        }
        assert!(alive(&app), "back");
        test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        let ahead = Vec2::new(-g.yaw.sin(), -g.yaw.cos());
        let to = (c - Vec2::new(g.position.x, g.position.z)).normalize();
        assert!(ahead.dot(to) > 0.99, "looking into the ring: {ahead:?} {to:?}");
    }

    /// Over the shoulder the camera stands behind, right of and above the eye, in the open; before a block behind it,
    /// nearer; never under the counter.
    #[test]
    fn over_the_shoulder_the_camera_stands_behind_and_keeps_out_of_walls() {
        let map = kitchen_sim::map::Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/kung-fu-dojo.json")).unwrap()).unwrap();
        let level = kitchen_sim::map::Level::new(map);
        let feet = Vec3::new(0., kitchen_sim::feast::FLOOR + 0.03, 2.78);
        let eye = shoulder(&level, feet, Quat::IDENTITY);
        let want = feet + Vec3::new(SIDE, PIVOT + RISE, BACK);
        assert!(eye.distance(want) < 1e-3, "{eye:?} {want:?}");
        // Looking up steeply, the camera would go under the counter: it stays above it.
        let low = shoulder(&level, feet, Quat::from_rotation_x(1.4));
        assert!(low.y >= level.ground(low.x as f64, low.z as f64) as f32 + CLEAR - 1e-4, "{low:?}");
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The death camera. When the hotdog being played dies the view leaves its eyes at once: within a
//! quarter of a second it is above and behind where the hotdog stood, looking down at its body,
//! and it circles slowly there until the next round puts it back on its feet. The dead have no
//! first person: nothing of it is drawn (the hands and the gun, [`blind`]) and the head-up display
//! keeps only the round's lines (`arena::update_hud`, `tags`). Nothing here changes a rule:
//! `arena::tick` goes on deciding who is alive and what the first-person view is, and this only
//! takes the camera over while the player is dead.
//!
//! The player can take the camera in hand. The mouse (with the pointer captured: while it is free the
//! first click only captures it, as in the round) or a finger dragged over a touch screen takes it round
//! whatever it watches, all the way round and up and down within limits ([`Orbit`]); the wheel or a pinch
//! brings it nearer or takes it further off. A click, Space or the touch Fire button shows the next living
//! hotdog ([`watchable`], [`next`]): in Forts the player's team mates, otherwise anybody. It starts on the
//! player's own body, and in Forts it goes to a team mate by itself after [`WAKE`] seconds. Left alone for
//! [`IDLE`] seconds it eases back into circling the body slowly or, watching a living hotdog, into
//! following it from behind. None of this happens at the card table, where nobody dies.
//!
//! * [`steer`] runs before `arena::tick`, which captures a free pointer at a click, so that it knows
//!   whether the pointer was captured before this frame's click; it keeps what the player's hand did
//!   ([`Steer`]).
//! * [`watch`] runs straight after `arena::tick`. It notes the view `tick` has just set (the
//!   first-person view while alive, which is where the camera leaves from), and, in network play,
//!   keeps this player's own third-person body: `network` builds one for everybody else, and
//!   offline `arena` already builds one for the player and shows it at death, so there is only
//!   ever the one. The body is the ordinary rig, posed every frame and hidden while alive, so
//!   `death` finds it like any other hotdog and hands it to a ragdoll when it dies.
//! * [`film`] runs once `death` has written the ragdoll for this frame: it picks whom to watch
//!   ([`DeathCam::choose`]) and aims the camera at that hotdog's torso. It leaves the camera alone if
//!   anything has moved it since `tick` (a pinned capture view).
//! * [`blind`] runs after it: while the player is dead the view camera (`viewmodel`) draws nothing.
//!
//! The camera keeps above the counter and inside the arena. Where a wall, a block or the edge of
//! the arena is in its way it comes in closer and higher ([`fits`]); where that is not enough the
//! circling turns back and goes round the other way, and the mouse stops at it ([`DeathCam::shoot`]).
//! Boxed in, it finds the nearest side there is room on, coming nearer if it was taken further off
//! than usual ([`settle`]). Under something (a den's roof: a table top, the sideboard, a chair's seat) with no room
//! as it is set up, it comes down as low as the mouse takes it, and nearer, and so looks out of the den's mouth
//! rather than down through its roof; and with no room even so it comes in as near as there is ([`pull_in`], which a
//! replay's follow camera uses too).
use crate::maya::HotdogCamera;
use crate::player_model::{Cues, Gait, PlayerBody, PlayerModels, Rig};
use crate::ragdoll::Scene;
use crate::viewmodel::{ViewCamera, VIEW_LAYER};
use crate::visuals::ArenaBounds;
use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use std::f32::consts::{PI, TAU};

/// Seconds the camera takes to get from the hotdog's eyes to its place over the body. Most of the
/// way is gone in the first tenth of a second: the dead do not linger in first person.
pub const PULL: f32 = 0.25;
/// Where the camera settles: this far behind the body and this far above it.
const BACK: f32 = 0.34;
const UP: f32 = 0.30;
/// How fast it then circles, in radians a second: once round in half a minute.
const TURN: f32 = 0.2;
/// The settled field of view; the camera eases to it from the first-person one.
const FOV: f32 = 0.873;
/// Never closer than this to the edge of the arena, and never lower than this above the counter.
const MARGIN: f32 = 0.03;
const HEAD: f32 = 0.10;
/// How far the camera has to be from the eyes it left before the body they belong to is shown.
const INSIDE: f32 = 0.06;
/// The middle of the sausage in the rig's torso frame.
const MIDDLE: Vec3 = Vec3::new(0., 0.098, 0.);
/// How high over its feet a standing hotdog's torso is, where its rig is not to hand.
const STANDING: f32 = 0.11;
/// How near the wheel brings the camera and how far off it takes it, in metres from what it watches (it
/// settles 0.45 m off).
pub const NEAR: f32 = 0.2;
pub const FAR: f32 = 1.0;
/// How low the mouse takes the camera and how high, as the angle it looks down at what it watches from,
/// in radians: about 9° and 72° (it settles at 41°).
pub const LOW: f32 = 0.15;
pub const HIGH: f32 = 1.25;
/// Radians the camera goes round, or up or down, for a count of the mouse (the first person turns 0.0022).
const SENS: f32 = 0.0025;
/// How much nearer a pixel of the wheel brings the camera, as a power of e, and the pixels a line of it
/// (a notch of a mouse wheel) counts as. A browser's notch, 100 px or 3 lines, brings it a seventh or a
/// fifth nearer; a native notch half a browser's.
const ZOOM: f32 = 0.0015;
const LINE: f32 = 50.;
/// Seconds without the mouse (or a drag, the wheel or a pinch) before the camera goes back to circling by
/// itself, and the seconds it takes to come back up to speed.
pub const IDLE: f32 = 3.;
const EASE: f32 = 1.;
/// Seconds after the death before the player's hand is heard: what the mouse was doing in the fight is not
/// taken for a turn of the camera, nor a last shot for a click to the next hotdog.
pub const HEARD: f32 = 0.5;
/// Forts: seconds a dead player watches its own body before it is shown its team mates.
pub const WAKE: f32 = 4.;
/// The furthest the camera goes round between two looks at whether there is room for it, in radians: the
/// mouse stops at whatever is in the way rather than jumping past it.
const STRIDE: f32 = 0.1;

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// An angle taken the short way round: between -π and π.
fn short(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// One frame of the death camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub eye: Vec3,
    pub turn: Quat,
    pub fov: f32,
}
impl Shot {
    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.eye).with_rotation(self.turn)
    }
}

/// How the camera is set up: how steeply it looks down at what it watches (`rise`, radians above the
/// level) and how far off it is (`reach`, metres). Every death starts from the settled view, `BACK`
/// behind and `UP` above, and the mouse and the wheel change it within their limits ([`Orbit::steer`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub rise: f32,
    pub reach: f32,
}
impl Default for Orbit {
    fn default() -> Self {
        Orbit { rise: UP.atan2(BACK), reach: BACK.hypot(UP) }
    }
}
impl Orbit {
    /// The mouse moved `lift` counts down (or a finger dragged as far), and the wheel or a pinch brought
    /// the camera `zoom` nearer (a power of e; below nothing it goes further off). Down raises the camera
    /// to look down more steeply, as the mouse moved down tips the first person's view down.
    pub fn steer(&mut self, lift: f32, zoom: f32) {
        self.rise = (self.rise + lift * SENS).clamp(LOW, HIGH);
        self.reach = (self.reach * (-zoom).exp()).clamp(NEAR, FAR);
    }
    /// The ways back from what it watches that the camera tries on a side, in turn: the one asked for,
    /// then each 12% of `BACK` nearer (scaled to how far off it is), and so higher, as far in as `BACK` ×
    /// 0.4 at the settled distance. Set up as it settles, these are the six steps it has always taken.
    fn backs(self) -> impl Iterator<Item = f32> {
        let scale = self.reach / Orbit::default().reach;
        let (step, least) = (BACK * 0.12 * scale, BACK * 0.4 * scale);
        let wanted = self.reach * self.rise.cos();
        let steps = ((wanted - least) / step + 1e-3).floor().max(0.) as usize;
        (0..=steps).map(move |k| wanted - k as f32 * step)
    }
}

/// Whether nothing of the map lies between the camera and the body, short of the body itself.
fn sight(scene: &Scene, eye: Vec3, body: Vec3) -> bool {
    (1..=8).all(|i| scene.clear(eye.lerp(body, i as f32 * 0.1), 0.012, 0.))
}

/// Where the camera is when it looks at a body from the compass direction `angle`, `back` behind
/// it and `reach` from it: the closer in it stands the higher it is. It never leaves the arena or
/// drops to the counter, and a body that went over the edge is watched from above the counter it left.
pub fn place(body: Vec3, angle: f32, back: f32, reach: f32, bounds: &ArenaBounds) -> Vec3 {
    let (low, high) = (bounds.min + MARGIN, bounds.max - MARGIN);
    let centre = Vec3::new(body.x.clamp(low.x, high.x), body.y.max(bounds.floor), body.z.clamp(low.y, high.y));
    let eye = centre + Vec3::new(angle.sin(), 0., angle.cos()) * back + Vec3::Y * (reach * reach - back * back).max(0.).sqrt();
    Vec3::new(
        eye.x.clamp(low.x, high.x),
        eye.y.clamp(bounds.floor + HEAD, (bounds.ceiling - 0.08).max(bounds.floor + HEAD)),
        eye.z.clamp(low.y, high.y),
    )
}

/// How far behind the body the camera can stand on the side `angle`, set up as `orbit`: as far as the
/// orbit asks if there is room, less (and so higher) if a wall, a block or the edge of the arena is in
/// the way, and `None` if that side is no good at all.
pub fn fits(body: Vec3, angle: f32, orbit: Orbit, bounds: &ArenaBounds, scene: &Scene) -> Option<f32> {
    let (low, high) = (bounds.min + MARGIN, bounds.max - MARGIN);
    orbit.backs().find(|back| {
        let eye = place(body, angle, *back, orbit.reach, bounds);
        // Not one that had to be pushed back inside.
        let wanted = body + Vec3::new(angle.sin(), 0., angle.cos()) * *back;
        let inside = wanted.x >= low.x - 1e-4 && wanted.x <= high.x + 1e-4 && wanted.z >= low.y - 1e-4 && wanted.z <= high.y + 1e-4;
        inside && scene.clear(eye, 0.03, 0.) && sight(scene, eye, body)
    })
}

/// The side nearest to `angle` that the camera can stand on, set up as `orbit`: the side, how far back
/// and how far off. Taken further off than usual and finding no room there, it comes nearer a step at a
/// time, as near as the usual distance. Boxed in on every side, it looks almost straight down from the
/// side the middle of the arena is on; and where that is inside something (the body is under a roof), it comes as low
/// as the mouse takes it, and nearer, to look out of the den's mouth, or failing that as near as there is room.
pub fn settle(body: Vec3, angle: f32, orbit: Orbit, bounds: &ArenaBounds, scene: &Scene) -> (f32, f32, f32) {
    let usual = Orbit::default().reach;
    let mut reach = orbit.reach;
    loop {
        let at = Orbit { reach, ..orbit };
        for step in 0..=10 {
            for side in [1., -1.] {
                let turned = angle + side * step as f32 * 0.3;
                if let Some(back) = fits(body, turned, at, bounds, scene) {
                    return (turned, back, reach);
                }
            }
        }
        if reach <= usual + 1e-4 {
            break;
        }
        reach = (reach * 0.8).max(usual);
    }
    let middle = bounds.centre() - body;
    let turned = middle.x.atan2(middle.z);
    let (back, reach) = (BACK * 0.3 * reach / usual, reach);
    // Boxed in on every side: almost straight down, from the side the middle of the arena is on.
    if scene.clear(place(body, turned, back, reach, bounds), 0.03, 0.) {
        return (turned, back, reach);
    }
    // That is inside something: the body is under a roof (a den's) with no room as the camera is set up. As low as the
    // mouse takes it, then nearer, looking out of the den's mouth.
    for low in [Orbit { rise: LOW, reach: orbit.reach.min(usual) }, Orbit { rise: LOW, reach: NEAR }] {
        for step in 0..=10 {
            for side in [1., -1.] {
                let turned = angle + side * step as f32 * 0.3;
                if let Some(back) = fits(body, turned, low, bounds, scene) {
                    return (turned, back, low.reach);
                }
            }
        }
    }
    // No room even so: low and as near as there is room, from the side the middle of the arena is on.
    let wanted = place(body, turned, NEAR * LOW.cos(), NEAR, bounds);
    let eye = pull_in(scene, body, wanted, 0.02);
    let near = Vec2::new(eye.x - body.x, eye.z - body.z).length().max(0.02);
    (turned, near, near.hypot((eye.y - body.y).max(0.)))
}

/// Where a camera that wants to stand at `eye`, looking at `at`, can: as far out along the line from `at` as nothing of
/// the map (`scene`) is in the way, `clear` short of it; `at` itself if something is right there.
pub fn pull_in(scene: &Scene, at: Vec3, eye: Vec3, clear: f32) -> Vec3 {
    let (way, far) = ((eye - at).normalize_or_zero(), at.distance(eye));
    if far < 1e-4 {
        return eye;
    }
    let steps = (far / 0.01).ceil() as usize;
    let mut room = 0.;
    for k in 1..=steps {
        let d = (k as f32 * 0.01).min(far);
        if !scene.clear(at + way * d, clear, 0.) {
            return at + way * (room - clear).max(0.);
        }
        room = d;
    }
    eye
}

/// What the player's hand did to the death camera in one frame ([`hand`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steer {
    /// How far the mouse moved (the pointer captured) or a finger dragged, in mouse counts: x to the
    /// right, y down.
    pub turn: Vec2,
    /// How much nearer the wheel or a pinch brought the camera, as a power of e (below nothing: further off).
    pub zoom: f32,
    /// A click with the pointer captured, Space or the touch Fire button: the next hotdog.
    pub next: bool,
}

/// What the player's hand did this frame. The mouse takes the camera round only with the pointer
/// `captured` (a click on a free pointer only captures it: `arena::tick`), the wheel is heard only while
/// the game has the keyboard (`focused`: the page's chat box takes it), and a drag, a pinch and the Fire
/// button come from the touch controls (`pad`, while they are showing). The wheel rolled up, away from the
/// player, brings the camera nearer.
pub fn hand(captured: bool, focused: bool, motion: Vec2, scroll: AccumulatedMouseScroll, click: bool, space: bool, pad: Option<&crate::touch::Pad>) -> Steer {
    let pixels = scroll.delta.y * if scroll.unit == MouseScrollUnit::Pixel { 1. } else { LINE };
    let pad = pad.filter(|pad| pad.on);
    Steer {
        turn: if captured { motion } else { Vec2::ZERO } + pad.map_or(Vec2::ZERO, |pad| pad.look),
        zoom: if focused { pixels * ZOOM } else { 0. } + pad.map_or(0., |pad| pad.pinch),
        next: (captured && click) || space || pad.is_some_and(|pad| pad.pulled),
    }
}

/// Who a dead player (`me`) may watch, by id: every other living hotdog, and in Forts only its living
/// team mates.
pub fn watchable(roster: &[kitchen_sim::feast::Player], me: u32, forts: bool) -> Vec<u32> {
    let team = roster.iter().find(|p| p.id == me).map(|p| p.team);
    roster.iter().filter(|p| p.alive && p.id != me && (!forts || Some(p.team) == team)).map(|p| p.id).collect()
}

/// The hotdog to watch after `now` among `among`: the one with the next id up, or round again to the
/// lowest. From the player's own body (`None`), or from one no longer among them (it has died), it is the
/// next one on. With nobody to watch it is the player's own body again.
pub fn next(now: Option<u32>, among: &[u32]) -> Option<u32> {
    let after = among.iter().copied().filter(|id| now.is_none_or(|now| *id > now)).min();
    after.or_else(|| among.iter().copied().min())
}

/// What the camera watches: the player's own body (`who` is `None`) or a living hotdog, with where its
/// torso is and the way that hotdog faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subject {
    pub who: Option<u32>,
    pub at: Vec3,
    pub yaw: f32,
}

/// The death camera's state, and this player's own body in network play.
#[derive(Resource, Default)]
pub struct DeathCam {
    /// Seconds since the player died; `None` while it is alive.
    since: Option<f32>,
    /// The first-person view on the last frame it was alive, and its field of view.
    first: Option<(Transform, f32)>,
    /// Which side of the body the camera is making for, and which way round it is circling.
    heading: f32,
    spin: f32,
    /// Where it is now: the side, how far back, how far off and what it looks at. Each follows its
    /// target a little behind, so neither a twitching ragdoll nor a block coming between them makes the
    /// picture jump.
    angle: f32,
    back: f32,
    reach: f32,
    body: Vec3,
    /// How the player has set the camera up, and the seconds since its hand last moved it.
    orbit: Orbit,
    idle: f32,
    /// What the player's hand did this frame ([`steer`]), whom it has chosen to watch (`None`: its own
    /// body), and whom the camera watched on the last frame.
    hand: Steer,
    whom: Option<u32>,
    filming: Option<u32>,
    /// What `arena::tick` left in the camera this frame.
    seen: Option<Transform>,
    /// Network play: the player's own third-person body, and whose it is.
    own: Option<(Entity, u32)>,
}
impl DeathCam {
    /// The player is alive and sees this: the camera is `arena::tick`'s, and a death would start here.
    pub fn alive(&mut self, first: Option<(Transform, f32)>) {
        self.since = None;
        self.first = first;
        self.whom = None;
    }
    /// How long the player has been dead.
    #[allow(dead_code)]
    pub fn since(&self) -> Option<f32> {
        self.since
    }
    /// Which living hotdog the dead player watches, if not its own body (for the head-up display).
    pub fn watching(&self) -> Option<u32> {
        self.since.and(self.whom)
    }
    /// What the player's hand did this frame ([`hand`]): heard by the next [`DeathCam::choose`] and
    /// [`DeathCam::shoot`].
    pub fn steer(&mut self, hand: Steer) {
        self.hand = hand;
    }
    /// Whether a shot is still taken from inside the hotdog's own head, where its body would
    /// only be seen from within.
    pub fn inside(&self, shot: &Shot) -> bool {
        self.first.is_some_and(|(first, _)| shot.eye.distance(first.translation) < INSIDE)
    }
    /// Whom the camera watches this frame, `among` the hotdogs that may be watched ([`watchable`]): the
    /// player's own body (`None`) to begin with, and the next ([`next`]) at a click, Space or the touch
    /// Fire button once the fight is behind it ([`HEARD`]). In Forts (`wake`) it goes to the first team
    /// mate by itself once the player has watched its own body that long. One who dies is passed over for
    /// the next, and with nobody left it is the player's own body again.
    pub fn choose(&mut self, among: &[u32], wake: Option<f32>) -> Option<u32> {
        let Some(t) = self.since else {
            self.whom = None;
            return None;
        };
        let gone = self.whom.is_some_and(|whom| !among.contains(&whom));
        let woken = self.whom.is_none() && wake.is_some_and(|wake| t >= wake);
        if (self.hand.next && t >= HEARD) || gone || woken {
            self.whom = next(self.whom, among);
        }
        self.whom
    }
    /// The player is dead, its body lies at `body` and it faced `yaw` when it died: moves the
    /// death camera on by `dt` and returns what it shows. (The tests' way in; `film` names whom it watches.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dead(&mut self, dt: f32, body: Vec3, yaw: f32, bounds: &ArenaBounds, scene: &Scene) -> Shot {
        self.shoot(dt, Subject { who: None, at: body, yaw }, bounds, scene)
    }
    /// Puts the camera behind `subject` at once (on the side it faces away from, or as near to that as
    /// there is room), set up as the player has it.
    fn cut(&mut self, subject: Subject, bounds: &ArenaBounds, scene: &Scene) {
        self.body = subject.at;
        let (heading, back, reach) = settle(subject.at, subject.yaw, self.orbit, bounds, scene);
        (self.heading, self.angle, self.back, self.reach, self.filming) = (heading, heading, back, reach, subject.who);
    }
    /// The player is dead and the camera watches `subject`: moves the death camera on by `dt`, as the
    /// player's hand this frame takes it, and returns what it shows.
    pub fn shoot(&mut self, dt: f32, subject: Subject, bounds: &ArenaBounds, scene: &Scene) -> Shot {
        let hand = std::mem::take(&mut self.hand);
        let t = match self.since {
            Some(t) => t + dt,
            None => {
                // Behind the hotdog, the way it was not looking, or as near to that as there is room: set up
                // as the camera settles, and circling.
                (self.orbit, self.idle, self.spin) = (Orbit::default(), IDLE + EASE, 1.);
                self.cut(subject, bounds, scene);
                0.
            }
        };
        self.since = Some(t);
        // Another hotdog to watch: the camera is behind it at once.
        if subject.who != self.filming {
            self.cut(subject, bounds, scene);
        }
        self.body += (subject.at - self.body) * (1. - (-dt * 8.).exp());
        // The player's hand, once the fight it died in is behind it.
        let hand = if t >= HEARD { hand } else { Steer::default() };
        if hand.turn != Vec2::ZERO || hand.zoom != 0. {
            self.orbit.steer(hand.turn.y, hand.zoom);
            self.idle = 0.;
        } else {
            self.idle += dt;
        }
        let orbit = self.orbit;
        // The mouse takes the camera round (moved right, the view turns right as the first person's does).
        // Left alone, it circles the body, starting as the camera gets there and again a while after the
        // hand let go, or it swings round behind the living hotdog it watches.
        let circling = hand.turn.x == 0. && subject.who.is_none();
        let ease = smooth((t - PULL * 0.5) / PULL) * smooth((self.idle - IDLE) / EASE);
        let by = if hand.turn.x != 0. {
            (-hand.turn.x * SENS).clamp(-PI, PI)
        } else if subject.who.is_some() {
            short(subject.yaw - self.heading) * (1. - (-dt * 2.).exp()) * ease
        } else {
            self.spin * TURN * dt * ease
        };
        // A stride at a time: where something is in the way the mouse stops at it and the circling turns
        // back, so the camera sweeps to and fro over the body rather than through a wall.
        let (mut found, mut left) = (None, by);
        loop {
            let stride = left.clamp(-STRIDE, STRIDE);
            let Some(back) = fits(self.body, self.heading + stride, orbit, bounds, scene) else {
                break;
            };
            (self.heading, found, left) = (self.heading + stride, Some(back), left - stride);
            if left.abs() < 1e-6 {
                break;
            }
        }
        let (back, reach) = match found {
            Some(back) => (back, orbit.reach),
            None => match fits(self.body, self.heading, orbit, bounds, scene) {
                Some(back) => {
                    if circling {
                        self.spin = -self.spin;
                    }
                    (back, orbit.reach)
                }
                // The body has moved and this side is no longer any good.
                None => {
                    let (heading, back, reach) = settle(self.body, self.heading, orbit, bounds, scene);
                    self.heading = heading;
                    (back, reach)
                }
            },
        };
        // In the player's hand the camera keeps up with it; left alone it takes its time.
        let quick = self.idle < IDLE;
        let (rate, most, follow) = if quick { (25., 12., 20.) } else { (6., 1.2, 5.) };
        let turn = short(self.heading - self.angle);
        self.angle += (turn * (1. - (-dt * rate).exp())).clamp(-most * dt, most * dt);
        let k = 1. - (-dt * follow).exp();
        self.back += (back - self.back) * k;
        self.reach += (reach - self.reach) * k;
        let mut stand = place(self.body, self.angle, self.back, self.reach, bounds);
        // Swung quickly round to where a block made it climb, it does not pass through the block on the way.
        if quick && !scene.clear(stand, 0.03, 0.) {
            (self.angle, self.back, self.reach) = (self.heading, back, reach);
            stand = place(self.body, self.angle, self.back, self.reach, bounds);
        }
        let onto = |eye: Vec3| Transform::from_translation(eye).looking_at(self.body, Vec3::Y).rotation;
        // Someone who joins already dead has no first-person view to leave.
        let Some((first, fov)) = self.first else {
            return Shot { eye: stand, turn: onto(stand), fov: FOV };
        };
        // The camera is knocked out of the hotdog's head at once and has all but arrived within a
        // tenth of a second; the view turns from where the hotdog was looking onto its body as fast,
        // the short way round, even when the only room for the camera is in front of it.
        let left = 1. - (t / PULL).clamp(0., 1.);
        let gone = 1. - left * left * left;
        let eye = first.translation.lerp(stand, gone);
        Shot { eye, turn: first.rotation.slerp(onto(eye), gone), fov: fov + (FOV - fov) * gone }
    }
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DeathCam>()
            .add_systems(Update, (steer.before(crate::arena::tick), watch.after(crate::arena::tick).before(crate::network::send)))
            .add_systems(
                PostUpdate,
                (film, blind)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// Before `arena::tick`, which captures a free pointer at a click: what the player's hand did this
/// frame ([`hand`]), for the death camera to hear if the player is dead.
fn steer(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    pad: Option<Res<crate::touch::Pad>>,
    windows: Query<(&Window, &CursorOptions)>,
    controls: (Option<Res<crate::controls::Bindings>>, Option<Res<crate::settings::Settings>>),
    mut cam: ResMut<DeathCam>,
) {
    let captured = windows.iter().any(|(_, cursor)| cursor.grab_mode == CursorGrabMode::Locked);
    let focused = windows.iter().any(|(window, _)| window.focused);
    // The trigger (the left button to begin with) and the jump (Space) as the player has them bound (controls.rs), and
    // the mouse at the player's sensitivity (settings.rs).
    let (bindings, settings) = (controls.0.map_or_else(crate::controls::Bindings::default, |b| b.clone()), controls.1.map_or_else(crate::settings::Settings::default, |s| *s));
    let (none_keys, none_mouse) = (ButtonInput::default(), ButtonInput::default());
    let (keys, mouse) = (keys.as_deref().unwrap_or(&none_keys), mouse.as_deref().unwrap_or(&none_mouse));
    let click = bindings.just(crate::controls::Action::Fire, keys, mouse);
    let space = bindings.just(crate::controls::Action::Jump, keys, mouse);
    let motion = motion.map_or(Vec2::ZERO, |motion| motion.delta) * settings.look();
    let scroll = scroll.map_or_else(AccumulatedMouseScroll::default, |scroll| *scroll);
    cam.steer(hand(captured, focused, motion, scroll, click, space, pad.as_deref()));
}

/// After [`film`]: while the player is dead (from the frame it dies, watching its body or another
/// hotdog) the view camera draws nothing, so no hand, gun or carried thing of the first person shows,
/// not even on the frame `arena::tick` posed it before it learnt of the death. The view camera still
/// finishes the picture (`viewmodel`), so only its layer is taken away, and it is given back on the
/// first frame the player is alive again.
fn blind(cam: Res<DeathCam>, mut views: Query<&mut RenderLayers, With<ViewCamera>>) {
    let wanted = if cam.since.is_some() { RenderLayers::none() } else { RenderLayers::layer(VIEW_LAYER) };
    for mut layers in &mut views {
        if *layers != wanted {
            *layers = wanted.clone();
        }
    }
}

/// What a hotdog's rig is told about the player it shows.
#[allow(clippy::needless_update)]
fn cues(player: &kitchen_sim::feast::Player) -> Cues {
    Cues {
        alive: player.alive,
        prone: player.prone,
        crouch: player.crouch,
        stim: player.stim,
        weapon: player.weapon,
        pitch: player.pitch,
        swings: player.swings,
        airborne: !player.grounded,
        launched: player.launched,
        velocity: Vec3::from_array(player.v),
        ..default()
    }
}

/// Straight after `arena::tick`: remembers the view it set, and keeps the player's own body in
/// network play.
pub(crate) fn watch(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|options| options.demo);
    let Some(game) = world.get_resource::<crate::arena::Game>() else {
        return;
    };
    let id = if game.network { game.network_id } else { 1 };
    let played = game.playing() && !demo;
    let (online, position, yaw) = (game.network, game.position, game.yaw);
    let me = game.roster.iter().find(|p| p.id == id).cloned();
    let view = world
        .query_filtered::<(&Transform, &Projection), With<HotdogCamera>>()
        .iter(world)
        .next()
        .map(|(transform, projection)| {
            let fov = match projection {
                Projection::Perspective(lens) => lens.fov,
                _ => FOV,
            };
            (*transform, fov)
        });
    world.resource_scope(|world, mut cam: Mut<DeathCam>| {
        cam.seen = view.map(|(transform, _)| transform);
        if !played {
            cam.alive(None);
        } else if me.as_ref().is_none_or(|p| p.alive) {
            cam.alive(view);
        }
        // Offline `arena::update_players` has a body for the player already.
        let wanted = me.filter(|_| played && online);
        if let Some((entity, whose)) = cam.own {
            if wanted.as_ref().is_none_or(|p| p.id != whose) || world.get_entity(entity).is_err() {
                if let Ok(body) = world.get_entity_mut(entity) {
                    body.despawn();
                }
                cam.own = None;
            }
        }
        let Some(player) = wanted else {
            return;
        };
        let at = if player.alive { position } else { Vec3::from_array(player.p) };
        let entity = match cam.own {
            Some((entity, _)) => entity,
            None => {
                let Some(models) = world.get_resource::<PlayerModels>().cloned() else {
                    return;
                };
                let entity = models.spawn(world, at, player.id);
                cam.own = Some((entity, player.id));
                entity
            }
        };
        if let Some(mut transform) = world.get_mut::<Transform>(entity) {
            transform.translation = at;
            transform.rotation = Quat::from_rotation_y(if player.alive { yaw } else { player.yaw });
        }
        // In first person there is nothing of oneself to see; dead, the body is what is watched; in Kung Fu over the
        // shoulder (fight.rs) it is seen fighting.
        let third = crate::fight::third_person(world);
        let titan = world.get_resource::<crate::arena::Game>().is_some_and(|g| crate::titan::drawn_as_titan(g, player.id));
        world.entity_mut(entity).insert(if (player.alive && !third) || player.consumed || titan {
            Visibility::Hidden
        } else {
            Visibility::Visible
        });
        crate::player_model::animate(world, entity, now, cues(&player));
    });
}

/// Once the ragdolls are written for this frame: while the player is dead, the camera is the
/// death camera's, on the player's own body or on the living hotdog it has chosen to watch.
#[allow(clippy::too_many_arguments)]
fn film(
    time: Res<Time<Real>>,
    game: Option<Res<crate::arena::Game>>,
    options: Option<Res<crate::runtime::Options>>,
    bounds: Option<Res<ArenaBounds>>,
    deaths: Option<Res<crate::death::Deaths>>,
    mut cam: ResMut<DeathCam>,
    bodies: Query<(Entity, &Gait, &Rig, &Transform), (With<PlayerBody>, Without<HotdogCamera>)>,
    joints: Query<&Transform, Without<HotdogCamera>>,
    mut shown: Query<&mut Visibility, With<PlayerBody>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<HotdogCamera>>,
    titans: Option<Res<crate::titan::Titans>>,
) {
    let Some(game) = game else {
        return;
    };
    if !game.playing() || options.is_some_and(|options| options.demo) || crate::cards::at_table(&game) {
        return;
    }
    let id = if game.network { game.network_id } else { 1 };
    let Some(player) = game.roster.iter().find(|p| p.id == id && !p.alive) else {
        return;
    };
    // A hotdog's torso where its rig has it (the ragdoll's, for the dead); failing that, where the
    // simulation has the hotdog.
    let torso = |who: u32| {
        let (_, _, rig, root) = bodies.iter().find(|(_, gait, ..)| gait.id() == who)?;
        let torso = joints.get(rig.joints().torso).ok()?;
        Some(root.transform_point(torso.translation + torso.rotation * MIDDLE))
    };
    let forts = game.fort.is_some();
    let whom = cam.choose(&watchable(&game.roster, id, forts), forts.then_some(WAKE));
    let subject = match whom.and_then(|whom| game.roster.iter().find(|p| p.id == whom)) {
        Some(p) => Subject { who: Some(p.id), at: torso(p.id).unwrap_or(Vec3::from_array(p.p) + Vec3::Y * STANDING), yaw: p.yaw },
        None => Subject { who: None, at: torso(id).unwrap_or(Vec3::from_array(player.p) + Vec3::Y * 0.03), yaw: player.yaw },
    };
    let bounds = bounds.map_or_else(ArenaBounds::default, |bounds| *bounds);
    let empty = Scene::default();
    let scene = deaths.as_ref().map_or(&empty, |deaths| &deaths.scene);
    let who = subject.who;
    let mut shot = cam.shoot(time.delta_secs().min(0.05), subject, &bounds, scene);
    // Watching the titan: from far enough off to see the giant whole (titan/camera.rs).
    if let Some(giant) = titans.as_deref().and_then(|t| t.drawn).filter(|(id, ..)| who == Some(*id)) {
        shot = crate::titan::camera::framed(shot, &giant.1.skeleton, &giant.2, &bounds);
    }
    // For the first frame the camera is still inside the body it is leaving.
    let own = bodies.iter().find(|(_, gait, ..)| gait.id() == id);
    if let Some(mut visibility) = own.filter(|_| cam.inside(&shot)).and_then(|(entity, ..)| shown.get_mut(entity).ok()) {
        *visibility = Visibility::Hidden;
    }
    for (mut transform, mut projection) in &mut cameras {
        // Something has taken the camera since `arena::tick` (a view pinned for a capture): leave it.
        if cam.seen.is_some_and(|seen| seen != *transform) {
            continue;
        }
        *transform = shot.transform();
        if let Projection::Perspective(lens) = &mut *projection {
            lens.fov = shot.fov;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::Map;

    const DT: f32 = 1. / 60.;
    fn stage() -> (ArenaBounds, Scene) {
        (ArenaBounds::default(), crate::death::scenery(&Map::bundled()))
    }
    /// The first-person view of a hotdog standing at `at` and facing `yaw`.
    fn eyes(at: Vec3, yaw: f32) -> (Transform, f32) {
        (Transform::from_translation(at + Vec3::Y * 0.135).with_rotation(Quat::from_rotation_y(yaw)), 85f32.to_radians())
    }
    /// Places all over the bundled counter where a hotdog can stand, corners and edges included.
    fn places() -> Vec<Vec3> {
        let feast = kitchen_sim::feast::Feast::new(false);
        let (bounds, _) = stage();
        let mut all = vec![];
        for i in 0..=36 {
            for j in 0..=14 {
                let (x, z) = (bounds.min.x + 0.03 + 3.54 * i as f32 / 36., bounds.min.y + 0.03 + 1.3 * j as f32 / 14.);
                let ground = feast.level.ground_at(x, z);
                if feast.level.walkable(x as f64, ground as f64, z as f64) {
                    all.push(Vec3::new(x, ground, z));
                }
            }
        }
        assert!(all.len() > 300, "{} places to stand", all.len());
        all
    }
    fn within(eye: Vec3, bounds: &ArenaBounds) -> bool {
        eye.x >= bounds.min.x && eye.x <= bounds.max.x && eye.z >= bounds.min.y && eye.z <= bounds.max.y
    }

    /// Where the camera settles when it wants to look from `angle`.
    fn perch(body: Vec3, angle: f32, bounds: &ArenaBounds, scene: &Scene) -> Vec3 {
        let (angle, back, reach) = settle(body, angle, Orbit::default(), bounds, scene);
        place(body, angle, back, reach, bounds)
    }
    /// The orbits the tests try: as it settles, and at the corners of the mouse's and the wheel's limits.
    fn orbits() -> [Orbit; 6] {
        let usual = Orbit::default();
        [
            usual,
            Orbit { rise: LOW, reach: NEAR },
            Orbit { rise: HIGH, reach: FAR },
            Orbit { rise: LOW, reach: FAR },
            Orbit { rise: HIGH, reach: NEAR },
            Orbit { rise: 0.5, reach: 0.7 },
        ]
    }
    /// Somewhere on the counter with room all round, as far back as it asks, for the camera as it settles, as
    /// low and as high as the mouse takes it and as near as the wheel brings it, and room along the counter
    /// (the long way) as far off as the wheel takes it.
    fn open() -> Vec3 {
        let (bounds, scene) = stage();
        let usual = Orbit::default();
        let room = |at: &Vec3| {
            let body = *at + Vec3::Y * 0.03;
            let full = |orbit: Orbit, angle: f32| fits(body, angle, orbit, &bounds, &scene).is_some_and(|back| (back - orbit.reach * orbit.rise.cos()).abs() < 1e-5);
            let round = [usual, Orbit { rise: LOW, ..usual }, Orbit { rise: HIGH, ..usual }, Orbit { reach: NEAR, ..usual }];
            round.iter().all(|orbit| (0..16).all(|turn| full(*orbit, turn as f32 * 0.4)))
                && [std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2].iter().all(|angle| full(Orbit { reach: FAR, ..usual }, *angle))
        };
        places().into_iter().find(room).expect("somewhere on the counter has room all round")
    }
    /// The compass direction the camera looks at `body` from, and how steeply it looks down.
    fn side(eye: Vec3, body: Vec3) -> (f32, f32) {
        let d = eye - body;
        (d.x.atan2(d.z), (d.y / d.length()).asin())
    }
    /// The mouse moved by `x` and `y` counts with the pointer captured.
    fn mouse(x: f32, y: f32) -> Steer {
        Steer { turn: Vec2::new(x, y), ..default() }
    }
    /// The touch controls, showing (`on`) or not, with a drag, a pinch and the trigger pressed or not.
    fn pad(on: bool, look: Vec2, pinch: f32, pulled: bool) -> crate::touch::Pad {
        let mut pad = crate::touch::Pad::default();
        (pad.on, pad.look, pad.pinch, pad.pulled, pad.fire) = (on, look, pinch, pulled, pulled);
        pad
    }
    /// A hotdog of the roster: its id, whether it lives and its team.
    fn hotdog(id: u32, alive: bool, team: u8) -> kitchen_sim::feast::Player {
        kitchen_sim::feast::Player { id, alive, team, ..default() }
    }

    #[test]
    fn the_camera_stands_inside_the_arena_above_the_counter_and_out_of_every_block() {
        let (bounds, scene) = stage();
        let (mut clear, mut all) = (0, 0);
        for at in places() {
            let body = at + Vec3::Y * 0.03;
            for turn in 0..16 {
                let eye = perch(body, turn as f32 * 0.4, &bounds, &scene);
                assert!(within(eye, &bounds), "{eye} is outside the arena, watching {at}");
                assert!(eye.y >= bounds.floor + HEAD - 1e-5 && eye.y < bounds.ceiling, "{eye} is not above the counter");
                assert!(eye.y > body.y + 0.25, "{eye} is not well above the body at {at}");
                assert!((eye.distance(body) - BACK.hypot(UP)).abs() < 0.02, "{eye} is not the usual distance from {at}");
                all += 1;
                clear += (scene.clear(eye, 0.03, 0.) && sight(&scene, eye, body)) as usize;
            }
        }
        // Wherever a hotdog can stand there is a side to watch it from; the exception on this map
        // is the sliver between the deck and the end of the counter, too narrow to stand in.
        assert!(clear * 1000 >= all * 985, "only {clear} of {all} views are clear");
    }

    #[test]
    fn in_the_open_it_looks_from_behind_and_above_and_against_a_wall_it_goes_round() {
        let (bounds, scene) = stage();
        let usual = Orbit::default();
        // Set up as it settles, the camera stands as it always has: `BACK` behind and `UP` above.
        assert!((usual.reach * usual.rise.cos() - BACK).abs() < 1e-6 && (usual.reach * usual.rise.sin() - UP).abs() < 1e-6);
        let open = places().into_iter().find(|at| {
            let body = *at + Vec3::Y * 0.03;
            (0..16).all(|turn| fits(body, turn as f32 * 0.4, usual, &bounds, &scene).is_some_and(|back| (back - BACK).abs() < 1e-5))
        });
        let open = open.expect("somewhere on the counter has room all round");
        let body = open + Vec3::Y * 0.03;
        let eye = perch(body, 0.9, &bounds, &scene);
        assert!(eye.distance(body + Vec3::new(0.9f32.sin() * BACK, UP, 0.9f32.cos() * BACK)) < 1e-5);
        // Its back to the far edge of the arena: the camera cannot go behind it, so it stands
        // higher at first and then round to the side, never outside.
        for gap in [0.25, 0.12, 0.05] {
            let body = Vec3::new(open.x, bounds.floor + 0.03, bounds.max.y - gap);
            if !scene.clear(body, 0.03, 0.) {
                continue;
            }
            let (angle, back, reach) = settle(body, 0., usual, &bounds, &scene);
            let eye = place(body, angle, back, reach, &bounds);
            assert!(eye.z <= bounds.max.y - MARGIN + 1e-5, "{eye} is through the wall");
            assert!((eye.distance(body) - BACK.hypot(UP)).abs() < 1e-4);
            assert!(back >= BACK * 0.39, "it does not hang straight overhead: {back}");
            assert!(eye.y - body.y >= UP - 1e-5 && (back < BACK - 1e-5 || angle != 0.));
        }
    }

    #[test]
    fn a_body_that_fell_off_the_counter_is_watched_from_above_it() {
        let (bounds, scene) = stage();
        let fallen = Vec3::new(bounds.min.x - 0.2, 0.03, bounds.min.y - 0.3);
        for turn in 0..16 {
            let eye = perch(fallen, turn as f32 * 0.4, &bounds, &scene);
            assert!(within(eye, &bounds) && eye.y >= bounds.floor + HEAD);
        }
    }

    #[test]
    fn the_view_leaves_first_person_at_once_then_circles_the_body_without_a_jump() {
        // A short ease at most: the dead do not linger in first person.
        assert!(PULL <= 0.3);
        let (bounds, scene) = stage();
        for at in places().into_iter().step_by(23) {
            let yaw = at.x * 3. + at.z;
            let body = at + Vec3::Y * 0.03;
            let mut cam = DeathCam::default();
            cam.alive(Some(eyes(at, yaw)));
            let (first, fov) = eyes(at, yaw);
            // The first frame is the first-person view, exactly (without its hands and gun: `blind`).
            let shot = cam.dead(0., body, yaw, &bounds, &scene);
            assert!(shot.eye.distance(first.translation) < 1e-6 && (shot.fov - fov).abs() < 1e-6);
            assert!(shot.transform().rotation.angle_between(first.rotation) < 1e-3, "the first frame looks elsewhere");
            assert!(cam.inside(&shot), "the body is not shown from inside its own head");
            let mut last = shot;
            let mut turned = 0.;
            for frame in 1..=600 {
                let shot = cam.dead(DT, body, yaw, &bounds, &scene);
                let t = frame as f32 * DT;
                // Quick, but a move, not a cut; once there, never a jump.
                let (moved, swung) = (shot.eye.distance(last.eye), shot.transform().rotation.angle_between(last.transform().rotation));
                let (most, widest) = if t > PULL + DT { (0.03, 0.12) } else { (0.15, 0.6) };
                assert!(moved < most, "the camera jumps {moved} at {t} over {at}");
                assert!(swung < widest, "the view snaps round {swung} at {t} over {at}");
                assert!(within(shot.eye, &bounds) && shot.eye.y > bounds.floor + 0.09, "{} at {t}", shot.eye);
                // Out of the head, and the body on show, from the second frame.
                assert!(frame < 2 || !cam.inside(&shot), "still inside the head at {t} over {at}");
                // Within a tenth of a second it is well above where the eyes were and turned to the body
                // (even where it has to go round to the far side), and a moment later looking right at it.
                let aim = (shot.turn * Vec3::NEG_Z).angle_between(body - shot.eye);
                if t >= 0.1 {
                    assert!(shot.eye.y > first.translation.y + 0.1 && aim < 0.5, "still near first person at {t} over {at}: {} {aim}", shot.eye);
                }
                assert!(t < 0.15 || aim < 0.2, "not yet looking at the body at {t} over {at}: {aim}");
                if t > PULL {
                    // Settled: above the body, looking straight at it, at the settled field of view.
                    let aim = (shot.turn * Vec3::NEG_Z).angle_between(body - shot.eye);
                    assert!(shot.eye.y > body.y + 0.2 && aim < 1e-3 && (shot.fov - FOV).abs() < 1e-4);
                    let (a, b) = (last.eye - body, shot.eye - body);
                    turned += Vec2::new(a.x, a.z).angle_to(Vec2::new(b.x, b.z)).abs();
                    assert!(Vec2::new(b.x, b.z).length() > BACK * 0.3, "it hangs straight over the body at {t} over {at}");
                }
                last = shot;
            }
            assert_eq!(cam.since().map(|t| (t * 10.).round()), Some(100.));
            assert!(!cam.inside(&last));
            // In ten seconds it has gone slowly part of the way round, or to and fro where it is hemmed in.
            assert!(turned > 0.5 && turned < 2.5, "it circled {turned} radians over {at}");
        }
    }

    #[test]
    fn it_follows_a_body_that_is_still_falling() {
        let (bounds, scene) = stage();
        let at = Vec3::new(0.2, bounds.floor, 3.0);
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, 0.)));
        let mut shot = cam.dead(0., at + Vec3::Y * 0.1, 0., &bounds, &scene);
        for frame in 0..240 {
            // Thrown a hand's breadth and dropped onto the counter.
            let t = (frame as f32 * DT).min(1.);
            let body = at + Vec3::new(0.07 * t, 0.1 - 0.07 * t, 0.03 * t);
            shot = cam.dead(DT, body, 0., &bounds, &scene);
        }
        let lying = at + Vec3::new(0.07, 0.03, 0.03);
        assert!((shot.turn * Vec3::NEG_Z).angle_between(lying - shot.eye) < 0.005);
    }

    #[test]
    fn a_new_round_gives_first_person_back_and_the_next_death_starts_from_it() {
        let (bounds, scene) = stage();
        let at = Vec3::new(0.2, bounds.floor, 3.0);
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, 0.3)));
        for frame in 0..200 {
            // (Taken round, nearer and higher, and watching another hotdog by the end.)
            if frame == 100 {
                cam.steer(Steer { turn: Vec2::new(300., 200.), zoom: 1., next: true });
                assert_eq!(cam.choose(&[2], None), Some(2));
            }
            cam.dead(DT, at + Vec3::Y * 0.03, 0.3, &bounds, &scene);
        }
        assert!(cam.since().is_some() && cam.watching() == Some(2) && cam.orbit != Orbit::default());
        // The round restarts somewhere else: `arena::tick` owns the camera again from that frame.
        let start = Vec3::new(-1.2, bounds.floor, 2.6);
        cam.alive(Some(eyes(start, 2.)));
        assert_eq!((cam.since(), cam.watching()), (None, None));
        let (first, fov) = eyes(start, 2.);
        // The next death starts on the player's own body, set up as the camera settles.
        assert_eq!(cam.choose(&[2], None), None);
        let shot = cam.dead(0., start + Vec3::Y * 0.03, 2., &bounds, &scene);
        assert_eq!(cam.since(), Some(0.));
        assert_eq!(cam.orbit, Orbit::default());
        assert!(shot.eye.distance(first.translation) < 1e-6 && (shot.fov - fov).abs() < 1e-6);
        assert!(shot.transform().rotation.angle_between(first.rotation) < 1e-3);
        // Outside a round there is no first person to return to, and nothing is filmed from one.
        cam.alive(None);
        let shot = cam.dead(0., at + Vec3::Y * 0.03, 0., &bounds, &scene);
        assert!((shot.turn * Vec3::NEG_Z).angle_between(at + Vec3::Y * 0.03 - shot.eye) < 1e-3 && shot.eye.y > at.y + 0.2);
    }

    #[test]
    fn the_mouse_takes_the_camera_all_the_way_round_and_up_and_down_within_limits() {
        // Down raises the camera to look down more steeply, a count at a time, as far as HIGH; up lowers it as
        // far as LOW. Neither changes how far off it is.
        let usual = Orbit::default();
        let mut orbit = usual;
        orbit.steer(100., 0.);
        assert!((orbit.rise - (usual.rise + 100. * SENS)).abs() < 1e-6 && orbit.reach == usual.reach);
        orbit.steer(1e5, 0.);
        assert_eq!(orbit.rise, HIGH);
        orbit.steer(-1e5, 0.);
        assert_eq!(orbit.rise, LOW);
        assert!(LOW > 0.1 && HIGH < 1.3 && LOW < usual.rise && usual.rise < HIGH);

        let (bounds, scene) = stage();
        let at = open();
        let body = at + Vec3::Y * 0.03;
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, 0.)));
        let mut shot = cam.dead(0., body, 0., &bounds, &scene);
        // What the mouse does in the fight the hotdog dies in is not heard.
        cam.steer(mouse(400., 400.));
        cam.dead(DT, body, 0., &bounds, &scene);
        assert!(cam.orbit == usual && cam.idle > IDLE, "heard straight after the death");
        for _ in 0..60 {
            shot = cam.dead(DT, body, 0., &bounds, &scene);
        }
        // Moved 200 counts right, the camera goes half a radian round the body the way that turns the view
        // right (as the first person turns), at once and without the circling.
        let (from, facing) = (cam.heading, shot.turn * Vec3::NEG_Z);
        cam.steer(mouse(200., 0.));
        cam.dead(DT, body, 0., &bounds, &scene);
        assert!((cam.heading - (from - 0.5)).abs() < 1e-5, "{} from {from}", cam.heading);
        for _ in 0..15 {
            shot = cam.dead(DT, body, 0., &bounds, &scene);
        }
        assert!(short(side(shot.eye, body).0 - cam.heading).abs() < 0.01, "not there a quarter of a second later");
        let now = shot.turn * Vec3::NEG_Z;
        assert!(facing.cross(now).y < 0. && (cam.heading - (from - 0.5)).abs() < 1e-5, "the view turned left, or the camera went on by itself");
        // All the way round, and on, in steps of a third of a radian: never a jump.
        let mut turned = 0.;
        let mut last = shot;
        for _ in 0..30 {
            cam.steer(mouse(-133., 0.));
            for _ in 0..8 {
                shot = cam.dead(DT, body, 0., &bounds, &scene);
                // (Moved left, the camera goes round the other way: the compass direction it looks from rises.)
                let step = short(side(shot.eye, body).0 - side(last.eye, body).0);
                assert!(step > -1e-4 && step < 0.2 && shot.eye.distance(last.eye) < 0.06, "a jump of {step}");
                turned += step;
                last = shot;
            }
        }
        assert!(turned > TAU + 2., "it went {turned} radians round");
        // Down as far as it goes: it looks down at the body from HIGH; up as far as it goes, from LOW, or as low as
        // the counter lets it.
        for (lift, low, high) in [(1e4, HIGH - 0.01, HIGH + 0.01), (-1e4, LOW - 0.01, LOW + 0.1)] {
            cam.steer(mouse(0., lift));
            for _ in 0..30 {
                shot = cam.dead(DT, body, 0., &bounds, &scene);
            }
            let (_, rise) = side(shot.eye, body);
            assert!(rise > low && rise < high, "it looks down from {rise} for {lift}");
            assert!((shot.turn * Vec3::NEG_Z).angle_between(body - shot.eye) < 0.01, "not looking at the body");
            assert!(shot.eye.y >= bounds.floor + HEAD - 1e-5);
        }
    }

    #[test]
    fn the_wheel_brings_the_camera_nearer_or_takes_it_further_off_within_limits() {
        // A notch of a mouse wheel rolled up (away from the player) brings it nearer; down takes it off. A
        // browser's 100 px is two native notches. Not heard while the chat box has the keyboard.
        let wheel = |unit, y: f32, focused| hand(true, focused, Vec2::ZERO, AccumulatedMouseScroll { unit, delta: Vec2::new(0., y) }, false, false, None).zoom;
        let notch = wheel(MouseScrollUnit::Line, 1., true);
        assert!(notch > 0.05 && notch < 0.1, "{notch}");
        assert!((wheel(MouseScrollUnit::Pixel, 100., true) - 2. * notch).abs() < 1e-6);
        assert!((wheel(MouseScrollUnit::Line, -1., true) + notch).abs() < 1e-6);
        assert_eq!(wheel(MouseScrollUnit::Line, 1., false), 0.);
        // As near as NEAR and as far off as FAR, and nothing else changes.
        let usual = Orbit::default();
        let mut orbit = usual;
        orbit.steer(0., notch);
        assert!((orbit.reach - usual.reach * (-notch).exp()).abs() < 1e-6 && orbit.rise == usual.rise);
        orbit.steer(0., 50.);
        assert_eq!(orbit.reach, NEAR);
        orbit.steer(0., -50.);
        assert_eq!(orbit.reach, FAR);
        assert!(NEAR < usual.reach && usual.reach < FAR);

        // A dead hotdog in the open, the camera along the counter (the way there is room to go far off).
        let (bounds, scene) = stage();
        let at = open();
        let body = at + Vec3::Y * 0.03;
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, std::f32::consts::FRAC_PI_2)));
        let mut shot = cam.dead(0., body, std::f32::consts::FRAC_PI_2, &bounds, &scene);
        for _ in 0..60 {
            shot = cam.dead(DT, body, std::f32::consts::FRAC_PI_2, &bounds, &scene);
        }
        let usual_distance = shot.eye.distance(body);
        assert!((usual_distance - usual.reach).abs() < 1e-3);
        for (zoom, near, far) in [(notch * 3., usual.reach * 0.75, usual.reach * 0.82), (40., NEAR - 1e-3, NEAR + 1e-3), (-40., FAR - 0.1, FAR + 1e-3)] {
            cam.steer(Steer { zoom, ..default() });
            for _ in 0..45 {
                shot = cam.dead(DT, body, std::f32::consts::FRAC_PI_2, &bounds, &scene);
            }
            let distance = shot.eye.distance(body);
            assert!(distance > near && distance < far, "{distance} off for {zoom}");
            assert!((shot.turn * Vec3::NEG_Z).angle_between(body - shot.eye) < 0.01, "not looking at the body");
            assert!(within(shot.eye, &bounds) && shot.eye.y < bounds.ceiling);
        }
    }

    #[test]
    fn a_click_or_space_shows_the_next_living_hotdog_passing_over_the_dead_and_round_again() {
        // From the player's own body to the lowest id, then up, then round again; one that died is passed over.
        let among = [5, 2, 3];
        assert_eq!(next(None, &among), Some(2));
        assert_eq!(next(Some(2), &among), Some(3));
        assert_eq!(next(Some(3), &among), Some(5));
        assert_eq!(next(Some(5), &among), Some(2), "round again");
        assert_eq!(next(Some(4), &among), Some(5), "the one watched has died");
        assert_eq!(next(Some(9), &among), Some(2));
        assert_eq!((next(None, &[]), next(Some(3), &[])), (None, None), "nobody left: its own body");
        assert_eq!(next(Some(3), &[3]), Some(3));
        // In a siege (or Free For All) any other living hotdog.
        let roster = [hotdog(1, false, 0), hotdog(2, true, 0), hotdog(3, false, 0), hotdog(4, true, 0), hotdog(5, true, 0)];
        assert_eq!(watchable(&roster, 1, false), [2, 4, 5]);
        // A click with the pointer captured, Space whether or not, or the touch Fire button. A click on a free
        // pointer only captures it (`arena::tick`).
        let pressed = |captured, click, space, pad: Option<crate::touch::Pad>| hand(captured, true, Vec2::ZERO, default(), click, space, pad.as_ref()).next;
        assert!(pressed(true, true, false, None));
        assert!(!pressed(false, true, false, None), "the first click captures the pointer");
        assert!(pressed(false, false, true, None) && pressed(true, false, true, None));
        assert!(!pressed(true, false, false, None));
        assert!(pressed(false, false, false, Some(pad(true, Vec2::ZERO, 0., true))));
        assert!(!pressed(false, false, false, Some(pad(false, Vec2::ZERO, 0., true))), "touch controls that are not showing");
        // The mouse turns the camera only with the pointer captured; a drag always.
        let turned = |captured, pad: Option<crate::touch::Pad>| hand(captured, true, Vec2::new(30., 4.), default(), false, false, pad.as_ref()).turn;
        assert_eq!((turned(true, None), turned(false, None)), (Vec2::new(30., 4.), Vec2::ZERO));
        assert_eq!(turned(false, Some(pad(true, Vec2::new(-7., 2.), 0.3, false))), Vec2::new(-7., 2.));
        assert_eq!(hand(false, true, Vec2::ZERO, default(), false, false, Some(&pad(true, Vec2::ZERO, 0.3, false))).zoom, 0.3);

        // Dead: its own body first, the next at a click once the fight is behind it, then on and round again.
        let (bounds, scene) = stage();
        let at = open();
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, 0.)));
        let mates = |ids: &[u32]| ids.iter().map(|id| Subject { who: Some(*id), at: at + Vec3::new(*id as f32 * 0.05 - 0.15, 0.11, 0.), yaw: *id as f32 }).collect::<Vec<_>>();
        let film = |cam: &mut DeathCam, among: &[u32], click: bool| {
            cam.steer(Steer { next: click, ..default() });
            let whom = cam.choose(among, None);
            let subject = mates(among).into_iter().find(|s| Some(s.who.unwrap()) == whom).unwrap_or(Subject { who: None, at: at + Vec3::Y * 0.03, yaw: 0. });
            (whom, cam.shoot(DT, subject, &bounds, &scene), subject)
        };
        let all = [2, 4, 5];
        assert_eq!(film(&mut cam, &all, true).0, None, "it starts on its own body");
        assert_eq!(film(&mut cam, &all, true).0, None, "a last shot in the fight is no click");
        for _ in 0..30 {
            film(&mut cam, &all, false);
        }
        let mut seen = vec![];
        for _ in 0..4 {
            let (whom, shot, subject) = film(&mut cam, &all, true);
            assert_eq!(cam.watching(), whom);
            // At once behind it (the side it faces away from: there is room) and looking at it.
            assert!(short(side(shot.eye, subject.at).0 - subject.yaw).abs() < 1e-3, "not behind {whom:?}");
            assert!((shot.turn * Vec3::NEG_Z).angle_between(subject.at - shot.eye) < 1e-3);
            seen.push(whom.unwrap());
            for _ in 0..10 {
                assert_eq!(film(&mut cam, &all, false).0, whom, "only a click moves it on");
            }
        }
        assert_eq!(seen, [2, 4, 5, 2]);
        // The one watched dies: the next. All dead: its own body.
        assert_eq!(film(&mut cam, &[4, 5], false).0, Some(4));
        assert_eq!(film(&mut cam, &[5], false).0, Some(5));
        assert_eq!(film(&mut cam, &[], false).0, None);
        assert_eq!(film(&mut cam, &[], true).0, None);
        assert_eq!(cam.watching(), None);
    }

    #[test]
    fn in_forts_only_team_mates_are_watched_the_first_of_them_after_four_seconds() {
        // Two teams of three: the player (1) and its mates 3 and 5, against 2, 4 and 6; 5 is dead.
        let roster = [hotdog(1, false, 0), hotdog(2, true, 1), hotdog(3, true, 0), hotdog(4, true, 1), hotdog(5, false, 0), hotdog(6, true, 1)];
        assert_eq!(watchable(&roster, 1, true), [3]);
        assert_eq!(watchable(&roster, 2, true), [4, 6]);
        assert_eq!(watchable(&roster, 1, false), [2, 3, 4, 6], "a siege or Free For All on the same roster: anybody");
        let mut roster = roster;
        roster[4].alive = true;
        let mates = watchable(&roster, 1, true);
        assert_eq!(mates, [3, 5]);
        // Its own body for four seconds, then a team mate by itself; a click shows the other and goes round.
        let (bounds, scene) = stage();
        let at = open();
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, 0.)));
        let mut whom = None;
        for frame in 0..=(WAKE / DT) as usize + 4 {
            whom = cam.choose(&mates, Some(WAKE));
            let t = cam.since().unwrap_or(0.);
            assert!(t >= WAKE || whom.is_none(), "{whom:?} at {t}");
            let at = whom.map_or(at + Vec3::Y * 0.03, |id| at + Vec3::new(id as f32 * 0.05 - 0.2, 0.11, 0.));
            cam.shoot(if frame == 0 { 0. } else { DT }, Subject { who: whom, at, yaw: 0. }, &bounds, &scene);
        }
        assert_eq!(whom, Some(3));
        let mut seen = vec![];
        for _ in 0..3 {
            cam.steer(Steer { next: true, ..default() });
            seen.push(cam.choose(&mates, Some(WAKE)).unwrap());
            cam.dead(DT, at, 0., &bounds, &scene);
        }
        assert_eq!(seen, [5, 3, 5]);
        // Its team mates all dead: its own body, and nothing to click to.
        cam.steer(Steer { next: true, ..default() });
        assert_eq!(cam.choose(&[], Some(WAKE)), None);
        // Anywhere but Forts there is no waking: its own body until a click.
        let mut cam = DeathCam::default();
        cam.dead(0., at, 0., &bounds, &scene);
        for _ in 0..(2. * WAKE / DT) as usize {
            assert_eq!(cam.choose(&[2, 3], None), None);
            cam.dead(DT, at, 0., &bounds, &scene);
        }
    }

    #[test]
    fn left_alone_for_three_seconds_it_goes_back_to_circling() {
        let (bounds, scene) = stage();
        let at = open();
        let body = at + Vec3::Y * 0.03;
        let mut cam = DeathCam::default();
        cam.alive(Some(eyes(at, 0.)));
        cam.dead(0., body, 0., &bounds, &scene);
        // Circling, as it always has.
        for _ in 0..120 {
            cam.dead(DT, body, 0., &bounds, &scene);
        }
        let mut last = cam.heading;
        cam.dead(DT, body, 0., &bounds, &scene);
        assert!(((cam.heading - last).abs() - TURN * DT).abs() < 1e-5, "not circling");
        // Taken in hand, it stays where the mouse left it for three seconds...
        cam.steer(mouse(40., 0.));
        cam.dead(DT, body, 0., &bounds, &scene);
        let held = cam.heading;
        let mut t = 0.;
        while t + DT < IDLE {
            cam.dead(DT, body, 0., &bounds, &scene);
            t += DT;
            assert!((cam.heading - held).abs() < 1e-6, "it moved by itself {t} s after the mouse");
        }
        // ...then eases back into circling, slowly at first, and is up to speed a second later.
        last = cam.heading;
        let mut steps = vec![];
        while t < IDLE + EASE + 0.5 {
            cam.dead(DT, body, 0., &bounds, &scene);
            t += DT;
            steps.push((cam.heading - last).abs());
            last = cam.heading;
        }
        assert!(steps[2] < TURN * DT * 0.05, "it did not ease in: {}", steps[2]);
        assert!(steps.windows(2).take((EASE / DT) as usize - 2).all(|w| w[1] >= w[0] - 1e-7), "it does not speed up smoothly");
        assert!(steps.iter().rev().take(20).all(|s| (s - TURN * DT).abs() < 1e-5), "not back up to speed: {:?}", steps.last());
        // A wheel or a pinch counts as the hand too.
        cam.steer(Steer { zoom: 0.2, ..default() });
        cam.dead(DT, body, 0., &bounds, &scene);
        let held = cam.heading;
        for _ in 0..60 {
            cam.dead(DT, body, 0., &bounds, &scene);
        }
        assert!((cam.heading - held).abs() < 1e-6);

        // Watching a living hotdog, left alone it comes round behind it, wherever it turns.
        let mut cam = DeathCam::default();
        cam.dead(0., body, 0., &bounds, &scene);
        for _ in 0..60 {
            cam.dead(DT, body, 0., &bounds, &scene);
        }
        let mate = |yaw| Subject { who: Some(2), at: body + Vec3::Y * 0.08, yaw };
        cam.steer(Steer { next: true, ..default() });
        cam.choose(&[2], None);
        cam.shoot(DT, mate(0.), &bounds, &scene);
        assert!(short(cam.heading).abs() < 1e-4, "not behind it");
        cam.steer(mouse(300., 0.));
        cam.shoot(DT, mate(0.), &bounds, &scene);
        let held = cam.heading;
        assert!((short(held) + 0.75).abs() < 1e-4);
        // It turns round; the camera stays where the mouse left it for three seconds, then follows.
        for _ in 0..(IDLE / DT) as usize - 5 {
            cam.shoot(DT, mate(1.5), &bounds, &scene);
            assert!((cam.heading - held).abs() < 1e-6);
        }
        for _ in 0..(6. / DT) as usize {
            cam.shoot(DT, mate(1.5), &bounds, &scene);
        }
        assert!(short(cam.heading - 1.5).abs() < 0.01, "not behind it: {}", cam.heading);
    }

    #[test]
    fn fits_keeps_the_camera_out_of_every_block_however_it_is_set_up() {
        let (bounds, scene) = stage();
        let usual = Orbit::default();
        // Every side it says there is room on, at every orbit: inside the arena, above the counter, out of every
        // block and with nothing between it and the body; and no further off than the orbit asks, nor nearer to the
        // vertical than the steepest step.
        let (mut sides, mut room) = (0, 0);
        for at in places().into_iter().step_by(3) {
            let body = at + Vec3::Y * 0.03;
            for orbit in orbits() {
                let backs: Vec<f32> = orbit.backs().collect();
                assert!(backs[0] == orbit.reach * orbit.rise.cos() && backs.windows(2).all(|w| w[1] < w[0]));
                assert!(*backs.last().unwrap() >= BACK * 0.4 * orbit.reach / usual.reach - 1e-5);
                for turn in 0..12 {
                    let angle = turn as f32 * 0.52;
                    sides += 1;
                    let Some(back) = fits(body, angle, orbit, &bounds, &scene) else {
                        continue;
                    };
                    room += 1;
                    let eye = place(body, angle, back, orbit.reach, &bounds);
                    assert!(within(eye, &bounds) && eye.y >= bounds.floor + HEAD - 1e-5 && eye.y < bounds.ceiling, "{eye} for {orbit:?} over {at}");
                    assert!(scene.clear(eye, 0.03, 0.) && sight(&scene, eye, body), "{eye} is in a block or behind one, for {orbit:?} over {at}");
                    // (Only the counter lifts it, a little, when it is asked to look at the body almost level.)
                    assert!(back <= orbit.reach * orbit.rise.cos() + 1e-5 && Vec2::new(eye.x - body.x, eye.z - body.z).length() <= back + 1e-4);
                    assert!(eye.distance(body) <= orbit.reach + 1e-4 || (eye.y - bounds.floor - HEAD).abs() < 1e-5, "{eye} for {orbit:?} over {at}");
                }
            }
        }
        assert!(room * 2 > sides, "room on only {room} of {sides} sides");
        // Settled anywhere at any orbit, it is in the arena and above the counter, no further off than asked and
        // no nearer than the usual distance unless it was asked to be.
        for at in places().into_iter().step_by(5) {
            let body = at + Vec3::Y * 0.03;
            for orbit in orbits() {
                let (angle, back, reach) = settle(body, at.x * 2., orbit, &bounds, &scene);
                let eye = place(body, angle, back, reach, &bounds);
                assert!(within(eye, &bounds) && eye.y >= bounds.floor + HEAD - 1e-5, "{eye} for {orbit:?} over {at}");
                // (Under something with no room as it is set up, it comes lower and nearer: `settle`.)
                let roofed = (1..=10).any(|i| !scene.clear(body + Vec3::Y * orbit.reach * i as f32 / 10., 0.03, 0.));
                assert!(reach <= orbit.reach + 1e-6 && (reach >= orbit.reach.min(usual.reach) - 1e-6 || roofed), "{reach} for {orbit:?} over {at}");
            }
        }
        // Taken round and up and down, nearer and further off, all over the counter, by a hand that does not let
        // up: never outside the arena, never down on the counter, and out of every block wherever there is room.
        let (mut clear, mut all) = (0, 0);
        let mut seed = 7u32;
        let mut roll = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2. - 1.
        };
        for at in places().into_iter().step_by(17) {
            let body = at + Vec3::Y * 0.03;
            let mut cam = DeathCam::default();
            cam.alive(Some(eyes(at, at.z)));
            cam.dead(0., body, at.z, &bounds, &scene);
            for frame in 1..=240 {
                if frame % 6 == 0 {
                    cam.steer(Steer { turn: Vec2::new(roll() * 300., roll() * 200.), zoom: roll() * 0.5, next: false });
                }
                let shot = cam.dead(DT, body, at.z, &bounds, &scene);
                if frame as f32 * DT > PULL + DT {
                    assert!(within(shot.eye, &bounds) && shot.eye.y >= bounds.floor + HEAD - 1e-5, "{} over {at}", shot.eye);
                    assert!((shot.turn * Vec3::NEG_Z).angle_between(cam.body - shot.eye) < 1e-3);
                    all += 1;
                    clear += scene.clear(shot.eye, 0.03, 0.) as usize;
                }
            }
        }
        assert!(clear * 1000 >= all * 990, "only {clear} of {all} frames out of the blocks");
    }

    /// The systems as the game runs them, around a stand-in for `arena::tick`: while the player is
    /// alive the camera is exactly what `tick` wrote.
    #[test]
    fn while_alive_the_camera_is_left_exactly_as_the_game_set_it() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<DeathCam>().add_systems(PostUpdate, (film, blind).chain());
        let first = eyes(Vec3::new(0.2, 0.9025, 3.0), 0.7);
        let camera = app
            .world_mut()
            .spawn((HotdogCamera, first.0, Projection::Perspective(PerspectiveProjection { fov: first.1, ..default() })))
            .id();
        // No game yet (the intro): nothing is filmed.
        app.update();
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), first.0);
        assert_eq!(app.world().resource::<DeathCam>().since(), None);
    }

    /// From the frame the player dies the view camera draws nothing of first person (its hands, its
    /// gun, what it carried), and it all comes back, exactly as it was, with the next life.
    #[test]
    fn nothing_of_first_person_is_drawn_while_dead_and_it_all_comes_back_alive() {
        let (bounds, scene) = stage();
        let mut app = App::new();
        app.init_resource::<DeathCam>().add_systems(PostUpdate, blind);
        let view = app.world_mut().spawn((ViewCamera, RenderLayers::layer(VIEW_LAYER))).id();
        let layers = |app: &App| app.world().get::<RenderLayers>(view).cloned();
        let at = Vec3::new(0.2, bounds.floor, 3.0);
        app.world_mut().resource_mut::<DeathCam>().alive(Some(eyes(at, 0.4)));
        app.update();
        assert_eq!(layers(&app), Some(RenderLayers::layer(VIEW_LAYER)), "alive, the hands and the gun are drawn");
        // It dies: on that very frame, and for as long as it stays dead, nothing.
        for frame in 0..120 {
            app.world_mut().resource_mut::<DeathCam>().dead(if frame == 0 { 0. } else { DT }, at + Vec3::Y * 0.03, 0.4, &bounds, &scene);
            app.update();
            assert_eq!(layers(&app), Some(RenderLayers::none()), "frame {frame} of death shows first person");
        }
        // The next round: first person again.
        app.world_mut().resource_mut::<DeathCam>().alive(Some(eyes(Vec3::new(-1.2, bounds.floor, 2.6), 2.)));
        app.update();
        assert_eq!(layers(&app), Some(RenderLayers::layer(VIEW_LAYER)));
        // Between rounds (no first person to return to) it is drawn as it always was too.
        app.world_mut().resource_mut::<DeathCam>().alive(None);
        app.update();
        assert_eq!(layers(&app), Some(RenderLayers::layer(VIEW_LAYER)));
    }

    /// The round as the game plays it, offline: dead in a siege, the mouse turns the death camera and not the
    /// hotdog, the wheel brings it nearer and changes no gun, the first click captures the pointer, and the clicks
    /// and Space after it show the other hotdogs in turn.
    #[test]
    fn dead_in_a_siege_the_mouse_turns_the_camera_not_the_hotdog_and_a_click_shows_the_next() {
        use crate::arena::{test_frame, Game};
        let mut app = crate::arena::test_app("");
        app.add_plugins(Plugin).add_systems(Update, crate::arena::tick);
        test_frame(&mut app, 0.05);
        test_frame(&mut app, 0.05);
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            assert!(g.playing() && g.roster.len() == 5 && g.roster.iter().all(|p| p.alive), "a siege of five, all alive");
            let me = &mut g.feast.players[0];
            (me.hp, me.alive) = (0., false);
        }
        let captured = |app: &mut App| app.world_mut().query::<&CursorOptions>().single(app.world()).unwrap().grab_mode == CursorGrabMode::Locked;
        let click = |app: &mut App| {
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            test_frame(app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            test_frame(app, 0.05);
        };
        for _ in 0..14 {
            test_frame(&mut app, 0.05);
        }
        assert!(app.world().resource::<DeathCam>().since().is_some_and(|t| t > HEARD), "dead");
        assert!(!captured(&mut app));
        click(&mut app);
        assert!(captured(&mut app), "the first click captures the pointer");
        assert_eq!(app.world().resource::<DeathCam>().watching(), None, "and only that");
        click(&mut app);
        assert_eq!(app.world().resource::<DeathCam>().watching(), Some(2));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        test_frame(&mut app, 0.05);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Space);
        test_frame(&mut app, 0.05);
        assert_eq!(app.world().resource::<DeathCam>().watching(), Some(3));
        // The camera is on that hotdog, looking at it.
        let camera = *app.world_mut().query_filtered::<&Transform, With<HotdogCamera>>().single(app.world()).unwrap();
        let three = Vec3::from_array(app.world().resource::<Game>().roster.iter().find(|p| p.id == 3).unwrap().p);
        assert!((camera.rotation * Vec3::NEG_Z).angle_between(three + Vec3::Y * 0.1 - camera.translation) < 0.2);
        assert!(camera.translation.distance(three) < 0.7);
        // The mouse takes the camera round and leaves the hotdog's own aim where it was; the wheel brings it nearer
        // and leaves the gun in hand alone.
        let (yaw, pitch, weapon) = {
            let g = app.world().resource::<Game>();
            (g.yaw, g.pitch, g.weapon)
        };
        let (heading, reach) = {
            let cam = app.world().resource::<DeathCam>();
            (cam.heading, cam.orbit.reach)
        };
        app.world_mut().resource_mut::<bevy::input::mouse::AccumulatedMouseMotion>().delta = Vec2::new(120., 40.);
        app.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0., 2.);
        test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert!(g.yaw == yaw && g.pitch == pitch && g.weapon == weapon, "the dead hotdog turned or changed guns");
        let cam = app.world().resource::<DeathCam>();
        assert!(short(cam.heading - heading) < -0.1 && cam.orbit.reach < reach, "the camera did not move: {} {}", cam.heading, cam.orbit.reach);
    }

    /// Invert Y (settings.rs) turns the death camera's climb over, as it does the first person's look: the mouse moved
    /// down raises the camera to look down more steeply, and turned over lowers it.
    #[test]
    fn invert_y_turns_the_death_cameras_climb_over() {
        use crate::arena::{test_frame, Game};
        let mut climbs = vec![];
        for invert in [false, true] {
            let mut app = crate::arena::test_app("");
            app.insert_resource(crate::settings::Settings { invert_y: invert, ..default() });
            app.add_plugins(Plugin).add_systems(Update, crate::arena::tick);
            test_frame(&mut app, 0.05);
            test_frame(&mut app, 0.05);
            {
                let mut g = app.world_mut().resource_mut::<Game>();
                let me = &mut g.feast.players[0];
                (me.hp, me.alive) = (0., false);
            }
            for _ in 0..14 {
                test_frame(&mut app, 0.05);
            }
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            test_frame(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            test_frame(&mut app, 0.05);
            let before = app.world().resource::<DeathCam>().orbit.rise;
            app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(0., 30.);
            test_frame(&mut app, 0.05);
            climbs.push(app.world().resource::<DeathCam>().orbit.rise - before);
        }
        assert!(climbs[0] > 0. && climbs[1] < 0. && (climbs[0] + climbs[1]).abs() < 1e-5, "{climbs:?}");
    }

}

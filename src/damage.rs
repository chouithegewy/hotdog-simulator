// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Where hits come from, and where this player's land: the damage indicators and the hit marker, round the crosshair.
//! No words.
//!
//! * **What is known** ([`Watch`]): every hotdog's record of what last hurt it (`feast::Player::last_damage`: the kind,
//!   the attacker, where it came from, the way it was going, when, and the HP it took), which offline is the round's
//!   own and online comes with every frame from the server (protocol 12 has always carried it). A record that changes
//!   is a hit; a hotdog alive last frame and dead now, with this player on its record, is this player's kill. Only the
//!   last hit of a step is on the record, so two in one step show as one.
//! * **Hits on this player** ([`Hurt`]): an arc on a ring round the crosshair that points at where the hit came from
//!   (ahead is up, behind is down), turning live as the player turns and walks; whole for WHOLE seconds and gone LIFE
//!   seconds after the last hit from there. Each source has its own arc (a hotdog by its id, anything else by where it
//!   is), brighter for more damage, popping out with a fresh hit. A round, a burst, a swarm or the turret is red; a blow,
//!   a backstab or a bite is a wider arc in a rosier red; a Kung Fu blow wide and orange-red; a hot griddle orange, at the
//!   bottom of the ring (it is from below). Hunger, a fall, and a hit from straight above or below have no way to point:
//!   the whole ring pulses, faintly. The edge of the screen nearest the source flashes a soft red as well. The settings
//!   turn all this off (settings.rs `damage_indicators`).
//! * **Hits on the giant** (Titan, this player the titan: [`Hurt::giant`]): it is under fire all match, and a hotdog's
//!   feedback would keep its screen red. Its rounds and blows are shown as the share of its health they take: one soft,
//!   faint cue for each eighth of the way round they come from, whoever fired ([`Hurt::giant_hit`]), out on a wide ring
//!   clear of the crosshair and, in its third person, of the giant itself ([`giant_radius`]), and no flash of the edges.
//!   A **weak-spot hit** is its own cue ([`Hurt::weak_hit`]): a thick gold arc pointing at the shooter (following it),
//!   with the weak spot's icon, and the edge on that side flashing gold; a **stagger** pulses every edge once, amber.
//! * **This player's hits** (the hit marker): four small white ticks round the crosshair when a shot or a blow of this
//!   player's hurts a hotdog; a kill (a hotdog, or a cat that adds to this player's kills) makes them bigger, thicker and
//!   red, and turns the crosshair itself into a red cross for a moment (`Game::mark_hit`). A backstab keeps its dagger
//!   beside them (whip.rs).
//! * **Kill slow motion** (slowmo.rs) is told of this player's kills, and of a death that ends the round.
//!
//! In the head-up display's look (hud.rs, icons.rs): white shapes with a dark rim, tinted, readable on the bright
//! counter, drawn for the pixels they take (sized from the window's height, 720 pixels being 1x). Offline and online
//! alike, and nothing here changes the round.
use crate::arena::Game;
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::{BackgroundGradient, ColorStop, Gradient, LinearGradient},
};
use kitchen_sim::feast::{self, Damage, Player};
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

/// Seconds an arc stays after the last hit from its source, and of those whole before it fades; seconds the ring's pulse
/// lasts, and the least between two (a steady drain pulses rather than glows); seconds the edge flashes.
pub const LIFE: f32 = 1.2;
pub const WHOLE: f32 = 0.25;
pub const RING_LIFE: f32 = 0.9;
const RING_EVERY: f32 = 0.45;
pub const FLASH: f32 = 0.5;
/// The most arcs at once (the oldest goes).
pub const ARCS: usize = 6;
/// Seconds the hit marker shows, and the kill's.
pub const HIT: f32 = 0.3;
pub const KILL: f32 = 0.65;
/// A record older than this (on the round's clock) when it is first seen is not news (whip.rs FRESH).
const FRESH: f32 = 1.;
/// Steeper than this (rise over run) a source is straight above or below.
const STEEP: f32 = 2.5;
/// Nearer than this (metres, flat) the source is where the hotdog is.
const NEAR: f32 = 0.02;
/// The seconds over which damage from one source is added up for its arc's brightness, and the HP that make it full.
const ADD_UP: f32 = 0.6;
const FULL: f32 = 40.;
/// The ring's outer radius, an arc's thickness (a narrow one, a wide one), how far its middle points out, the ring's own
/// thickness and each arc's half angle: at SCALE (hud.rs), in pixels and radians.
const RADIUS: f32 = 66.;
const THICK: [f32; 2] = [11., 12.];
const PEAK: f32 = 4.5;
const RING_THICK: f32 = 3.5;
const SPREAD: [f32; 2] = [0.4, 0.86];
/// The hit marker's ticks: from and to (pixels from the middle at SCALE) and their width; the kill's.
const TICKS: [(f32, f32, f32); 2] = [(7., 13., 2.6), (6., 18.5, 4.)];
/// The edges' flash at its brightest, and how far in it reaches (a share of the screen).
const EDGE: f32 = 0.5;
const EDGE_IN: [f32; 2] = [22., 30.];
/// Titan, the giant's own feedback: the share of its full health that, taken within ADD_UP, makes a cue as heavy as FULL
/// makes a hotdog's; the wide ring its cues stand on, as a share of the window's smaller side; each cue's half angle and
/// thickness (at SCALE), soft and weak-spot; and the strength of a stagger's pulse round the edges.
pub const GIANT_SHARE: f32 = 0.03;
pub const GIANT_RING: f32 = 0.36;
const GIANT_SPREAD: [f32; 2] = [0.2, 0.17];
const GIANT_THICK: [f32; 2] = [6., 13.];
const STAGGER_PULSE: f32 = 0.6;
const WEAK_EDGE: f32 = 0.6;
/// The weak spot's gold (hud.rs), and a stagger's amber.
pub const WEAK_RGB: [f32; 3] = [1.0, 0.84, 0.3];
pub const STAGGER_RGB: [f32; 3] = [1.0, 0.62, 0.18];

/// What hurt, by its colour.
pub const SHOT_RGB: [f32; 3] = [1.0, 0.2, 0.16];
pub const BLOW_RGB: [f32; 3] = [1.0, 0.34, 0.5];
pub const FU_RGB: [f32; 3] = [1.0, 0.42, 0.2];
pub const BURN_RGB: [f32; 3] = [1.0, 0.56, 0.1];
pub const PEST_RGB: [f32; 3] = [0.62, 0.86, 0.4];
pub const HUNGER_RGB: [f32; 3] = [1.0, 0.72, 0.26];
/// The hit marker's, and the kill's.
pub const MARK_RGB: [f32; 3] = [0.97, 0.97, 0.93];
pub const KILL_RGB: [f32; 3] = [1.0, 0.16, 0.12];
/// The rim's colour (icons.rs's ink) and how opaque.
const INK: [f32; 3] = [0.035, 0.04, 0.05];
const INK_ALPHA: f32 = 0.9;

// ----- What happened.

/// One thing the records tell of since the last frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// This player was hurt: its record.
    Hurt(Damage),
    /// A hotdog this player hurt (and killed, `kill`), and where its chest is.
    Dealt { victim: u32, kill: bool, at: Vec3 },
    /// This player's kills went up with no hotdog's death of its doing to show for it (a cat).
    Kills,
    /// A death ended the round: whose, and where.
    Final { victim: u32, at: Vec3 },
}
/// A record as it was last seen: its kind, attacker, time and HP.
type Tag = (u8, u32, u32, u32);
fn tag(d: &Damage) -> Tag {
    (d.kind, d.attacker, d.time.to_bits(), d.amount.to_bits())
}
/// The records as they were last frame, to tell what is new.
#[derive(Clone, Debug, Default)]
pub struct Watch {
    seen: HashMap<u32, (Tag, bool)>,
    kills: Option<u32>,
    phase: &'static str,
    /// The last death seen: the round's clock then, whose, where.
    died: Option<(f32, u32, Vec3)>,
}
impl Watch {
    /// What the records of `roster` (the round's clock `elapsed`, its phase `phase`) tell of since the last call, for
    /// player `me`. A hotdog seen for the first time tells nothing (it was there before this client was).
    pub fn hear(&mut self, me: u32, roster: &[Player], elapsed: f32, phase: &'static str) -> Vec<Event> {
        let mut out = vec![];
        for p in roster {
            let record = p.last_damage;
            let Some((was, alive)) = self.seen.insert(p.id, (tag(&record), p.alive)) else { continue };
            let fell = alive && !p.alive;
            if fell {
                self.died = Some((elapsed, p.id, chest(p)));
            }
            let news = tag(&record) != was && record.kind != 0 && (elapsed - record.time).abs() <= FRESH;
            if !news {
                continue;
            }
            if p.id == me {
                out.push(Event::Hurt(record));
            } else if record.attacker == me && me != 0 {
                out.push(Event::Dealt { victim: p.id, kill: fell, at: chest(p) });
            }
        }
        if let Some(mine) = roster.iter().find(|p| p.id == me) {
            if self.kills.is_some_and(|before| mine.kills > before) && !out.iter().any(|e| matches!(e, Event::Dealt { kill: true, .. })) {
                out.push(Event::Kills);
            }
            self.kills = Some(mine.kills);
        }
        // The round over, with a death just before it (or with it).
        if phase != self.phase && phase == "RoundOver" && !self.phase.is_empty() {
            if let Some((_, victim, at)) = self.died.filter(|(t, ..)| (elapsed - t).abs() < 0.5) {
                out.push(Event::Final { victim, at });
            }
        }
        self.phase = phase;
        self.seen.retain(|id, _| roster.iter().any(|p| p.id == *id));
        out
    }
}
/// Where a hotdog's chest is (as the simulation reckons it: lower crouched, lower still prone, and tucked in the air
/// where it stands).
pub fn chest(p: &Player) -> Vec3 {
    feast::chest(p)
}

// ----- Which way.

/// How a hit is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cue {
    /// From a place: an arc that points at it (at the hotdog `follow` while it lives, if not 0); `wide` for a blow.
    From { at: Vec3, follow: u32, wide: bool },
    /// From under the feet (a hot griddle): an arc at the bottom of the ring.
    Below,
    /// From nowhere in particular (hunger, a fall), or straight above or below: the whole ring.
    Around,
}
/// How the hit on `record` is shown to the hotdog whose chest is at `chest`.
pub fn cue(record: &Damage, chest: Vec3) -> Cue {
    // (A round in the face is its gun's.)
    let record = &Damage { kind: feast::plain_kind(record.kind), ..*record };
    match record.kind {
        feast::BURN => return Cue::Below,
        feast::HUNGER | feast::FALL => return Cue::Around,
        _ => {}
    }
    let wide = matches!(record.kind, feast::MELEE | feast::BACKSTAB | feast::ZOMBIE | kitchen_sim::kungfu::BLOW);
    // A hotdog's round or blow points at that hotdog while it lives; a burst, a swarm, a cat, the turret where they were.
    let follows = matches!(record.kind, 1 | 2 | 3 | 5 | feast::SYRUP | feast::MAYONNAISE | feast::MELEE | feast::BACKSTAB | kitchen_sim::kungfu::BLOW);
    let mut at = Vec3::from_array(record.p);
    let off = at - chest;
    let mut flat = Vec2::new(off.x, off.z).length();
    if flat < NEAR {
        // Right where the hotdog is (a glob bursting on it, a swarm round it): back along the way the hit was going.
        let d = Vec3::from_array(record.d);
        let Some(back) = Vec2::new(-d.x, -d.z).try_normalize().filter(|_| d.x.hypot(d.z) > 0.3) else {
            return Cue::Around;
        };
        at = chest + Vec3::new(back.x, 0., back.y) * 0.5;
        flat = 0.5;
    }
    if (at.y - chest.y).abs() > STEEP * flat {
        return Cue::Around;
    }
    Cue::From { at, follow: if follows { record.attacker } else { 0 }, wide }
}
/// The colour of a hit of `kind`.
pub fn tint(kind: u8) -> [f32; 3] {
    match kind {
        feast::MELEE | feast::BACKSTAB | feast::ZOMBIE => BLOW_RGB,
        kitchen_sim::kungfu::BLOW => FU_RGB,
        feast::BURN => BURN_RGB,
        feast::PESTS => PEST_RGB,
        feast::HUNGER => HUNGER_RGB,
        _ => SHOT_RGB,
    }
}
/// Where `at` is round the crosshair of a hotdog at `eye` looking along `yaw`: radians clockwise from straight up the
/// screen (ahead is up, to the right is right, behind is down).
pub fn angle(eye: Vec3, yaw: f32, at: Vec3) -> f32 {
    crate::hud::bearing(eye, yaw, at).0
}
/// How opaque an arc is `age` seconds after the last hit from its source: whole, then fading out by LIFE.
pub fn fade(age: f32) -> f32 {
    if age <= WHOLE { 1. } else { 1. - smooth((age - WHOLE) / (LIFE - WHOLE)) }
}
/// How big an arc is `age` seconds after a fresh hit popped it.
pub fn pop(age: f32) -> f32 {
    1. + 0.14 * (-age.max(0.) / 0.07).exp()
}
/// How bright the edge's flash is `age` seconds after a fresh hit.
pub fn flash(age: f32) -> f32 {
    if (0. ..FLASH).contains(&age) { (1. - age / FLASH).powi(2) } else { 0. }
}
/// The ring's pulse `age` seconds in: how opaque (faint) and how big.
pub fn ring_alpha(age: f32) -> f32 {
    if (0. ..RING_LIFE).contains(&age) { 0.6 * (1. - age / RING_LIFE).powf(1.5) } else { 0. }
}
pub fn ring_scale(age: f32) -> f32 {
    0.93 + 0.14 * (age / RING_LIFE).clamp(0., 1.).sqrt()
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// One source's arc.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    /// Which source: the hotdog that did it (0 for none), the way it lies from this player when there is none (an
    /// eighth of the way round), and the hurt's sort.
    pub key: (u32, i32, u8),
    /// Where it is, and the hotdog it follows (0: it stays where it was).
    pub at: Vec3,
    pub follow: u32,
    pub wide: bool,
    /// At the bottom of the ring whichever way the player looks (a hot griddle).
    pub below: bool,
    pub tint: [f32; 3],
    /// The real time of the last hit from it, and of the last that popped it.
    pub born: f32,
    pub popped: f32,
    /// The HP it took lately (added up over ADD_UP; the giant's, scaled to a hotdog's: [`Hurt::giant_hit`]).
    pub hurt: f32,
    /// Titan: a weak-spot hit on the giant (gold, thick, with the icon, and the edge flashes).
    pub weak: bool,
}
impl Arc {
    /// Where it stands round the crosshair (see [`angle`]).
    pub fn angle(&self, eye: Vec3, yaw: f32) -> f32 {
        if self.below { PI } else { angle(eye, yaw, self.at) }
    }
    /// How heavy it is lately, 0.4 to 1.
    pub fn weight(&self) -> f32 {
        (0.4 + 0.6 * self.hurt / FULL).min(1.)
    }
    /// How opaque it is at `now`.
    pub fn alpha(&self, now: f32) -> f32 {
        fade(now - self.born) * (0.55 + 0.45 * self.weight())
    }
    /// How opaque a cue of the giant's is at `now`: a soft one faint (more so for a little damage), a weak-spot hit whole.
    pub fn giant_alpha(&self, now: f32) -> f32 {
        fade(now - self.born) * if self.weak { 1. } else { 0.12 + 0.5 * (self.hurt / FULL).min(1.) }
    }
}
/// Titan: the radius (logical pixels) of the ring the giant's cues stand on, in a window `w` by `h`.
pub fn giant_radius(w: f32, h: f32) -> f32 {
    GIANT_RING * w.min(h)
}
/// The whole ring's pulse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub born: f32,
    pub tint: [f32; 3],
}

/// The indicators and the hit marker as they stand, and what is on the screen for them.
#[derive(Resource, Default)]
pub struct Hurt {
    pub arcs: Vec<Arc>,
    pub ring: Option<Ring>,
    /// The hit marker: the real time of the hit, and whether it was a kill.
    pub marker: Option<(f32, bool)>,
    pub watch: Watch,
    /// What the records told this frame (a replay's recorded view keeps it: replay/pov.rs).
    pub heard: Vec<Event>,
    /// Titan: the giant's full health while this player is the giant (None: a hotdog's feedback, as always).
    pub giant: Option<f32>,
    /// Titan: the newest of the giant's events heard (`titan::View::events` are numbered), and a stagger's pulse.
    pub told: u32,
    pub pulse: Option<Ring>,
    ui: Option<Ui>,
}
impl Hurt {
    /// A hit on this player, by its record, whose chest is at `chest`, at real time `now`.
    pub fn hit(&mut self, record: &Damage, chest: Vec3, now: f32) {
        let record = &Damage { kind: feast::plain_kind(record.kind), ..*record };
        let amount = record.amount.max(0.);
        let tint = tint(record.kind);
        let (at, follow, wide, below) = match cue(record, chest) {
            Cue::Around => {
                if self.ring.is_none_or(|r| now - r.born >= RING_EVERY) {
                    self.ring = Some(Ring { born: now, tint });
                }
                return;
            }
            Cue::Below => (chest, 0, true, true),
            Cue::From { at, follow, wide } => (at, follow, wide, false),
        };
        let way = Vec2::new(at.x - chest.x, at.z - chest.z);
        let sector = if follow != 0 || below { 0 } else { (way.x.atan2(way.y) / (TAU / 8.)).round() as i32 & 7 };
        let sort = match record.kind {
            feast::BURN => 1,
            feast::PESTS => 2,
            feast::ZOMBIE => 3,
            feast::TURRET => 4,
            _ => 0,
        };
        let key = (record.attacker, sector, sort);
        match self.arcs.iter_mut().find(|a| a.key == key) {
            Some(a) => {
                a.hurt = a.hurt * (-(now - a.born).max(0.) / ADD_UP).exp() + amount;
                if now - a.popped > 0.25 {
                    a.popped = now;
                }
                (a.at, a.follow, a.wide, a.below, a.tint, a.born) = (at, follow, wide, below, tint, now);
            }
            None => {
                self.arcs.push(Arc { key, at, follow, wide, below, tint, born: now, popped: now, hurt: amount, weak: false });
                self.trim();
            }
        }
    }
    /// At most ARCS: the oldest goes.
    fn trim(&mut self) {
        if self.arcs.len() > ARCS {
            let oldest = self.arcs.iter().enumerate().min_by(|a, b| a.1.born.total_cmp(&b.1.born)).map(|(i, _)| i).unwrap();
            self.arcs.remove(oldest);
        }
    }
    /// Titan: a round or a blow on the giant (this player; `max` its full health), by its record, its chest at `chest`:
    /// one soft cue for each eighth of the way round, whoever fired, as heavy as the share of its health taken from there
    /// lately (GIANT_SHARE of it is a hotdog's FULL). What has no way to point (a fall) shows nothing.
    pub fn giant_hit(&mut self, record: &Damage, chest: Vec3, now: f32, max: f32) {
        let record = &Damage { kind: feast::plain_kind(record.kind), ..*record };
        let Cue::From { at, .. } = cue(record, chest) else {
            return;
        };
        let amount = record.amount.max(0.) / (max.max(1.) * GIANT_SHARE) * FULL;
        let sector = Self::sector(at - chest);
        let key = (0, sector, 0);
        match self.arcs.iter_mut().find(|a| a.key == key) {
            Some(a) => {
                a.hurt = a.hurt * (-(now - a.born).max(0.) / ADD_UP).exp() + amount;
                (a.at, a.born) = (at, now);
            }
            None => {
                self.arcs.push(Arc { key, at, follow: 0, wide: true, below: false, tint: SHOT_RGB, born: now, popped: now, hurt: amount, weak: false });
                self.trim();
            }
        }
    }
    /// Titan: a weak-spot hit on the giant by `by` (its chest at `from`, followed while it lives), `amount` of its
    /// health: a gold cue of its own pointing at the shooter, popping, and the edge on that side flashing gold.
    pub fn weak_hit(&mut self, by: u32, from: Vec3, chest: Vec3, now: f32, amount: f32) {
        let key = (by, Self::sector(from - chest), 9);
        match self.arcs.iter_mut().find(|a| a.key == key) {
            Some(a) => {
                a.hurt = a.hurt * (-(now - a.born).max(0.) / ADD_UP).exp() + amount;
                (a.at, a.born, a.popped) = (from, now, now);
            }
            None => {
                self.arcs.push(Arc { key, at: from, follow: by, wide: true, below: false, tint: WEAK_RGB, born: now, popped: now, hurt: amount.max(FULL), weak: true });
                self.trim();
            }
        }
    }
    /// Titan: the giant staggers: every edge pulses once, amber.
    pub fn stagger(&mut self, now: f32) {
        self.pulse = Some(Ring { born: now, tint: STAGGER_RGB });
    }
    /// Which eighth of the way round a source lies in, `way` from the chest.
    fn sector(way: Vec3) -> i32 {
        (way.x.atan2(way.z) / (TAU / 8.)).round() as i32 & 7
    }
    /// A hit of this player's at real time `now` (`kill`: it killed). A kill's marker is not cut short by a hit.
    pub fn mark(&mut self, now: f32, kill: bool) {
        if !kill && self.marker.is_some_and(|(at, k)| k && now - at < KILL) {
            return;
        }
        self.marker = Some((now, kill));
    }
    /// Forgets what is past at `now`, and moves the arcs that follow a hotdog to where it is in `roster`.
    pub fn age(&mut self, roster: &[Player], now: f32) {
        self.arcs.retain(|a| now - a.born < LIFE);
        for a in &mut self.arcs {
            if let Some(p) = roster.iter().find(|p| p.id == a.follow && a.follow != 0 && p.alive) {
                a.at = chest(p);
            }
        }
        if self.ring.is_some_and(|r| now - r.born >= RING_LIFE) {
            self.ring = None;
        }
        if self.pulse.is_some_and(|r| now - r.born >= FLASH) {
            self.pulse = None;
        }
        if self.marker.is_some_and(|(at, kill)| now - at >= if kill { KILL } else { HIT }) {
            self.marker = None;
        }
    }
    /// The edges' flash at `now` for a player at `eye` looking along `yaw`: top, right, bottom, left, each its colour and
    /// how opaque.
    pub fn edges(&self, eye: Vec3, yaw: f32, now: f32) -> [([f32; 3], f32); 4] {
        let mut sum = [([0f32; 3], 0f32); 4];
        let mut add = |side: usize, tint: [f32; 3], a: f32| {
            if a > 0. {
                let (c, total) = &mut sum[side];
                for k in 0..3 {
                    c[k] += tint[k] * a;
                }
                *total += a;
            }
        };
        // (The giant's soft cues never flash the edges: only a weak-spot hit does.)
        for arc in self.arcs.iter().filter(|a| self.giant.is_none() || a.weak) {
            // (The giant's nape can be hit again and again: its gold flash is a pulse at the edge, not a wash.)
            let strength = flash(now - arc.popped) * (0.5 + 0.5 * arc.weight()) * if arc.weak { WEAK_EDGE } else { 1. };
            let theta = arc.angle(eye, yaw);
            let (c, s) = (theta.cos(), theta.sin());
            for (side, share) in [c.max(0.), s.max(0.), (-c).max(0.), (-s).max(0.)].into_iter().enumerate() {
                add(side, arc.tint, strength * share * share);
            }
        }
        if let Some(r) = self.ring {
            for side in 0..4 {
                add(side, r.tint, 0.4 * flash(now - r.born));
            }
        }
        if let Some(r) = self.pulse {
            for side in 0..4 {
                add(side, r.tint, STAGGER_PULSE * flash(now - r.born));
            }
        }
        sum.map(|(c, total)| if total > 0. { (c.map(|v| v / total), (total * EDGE).min(EDGE)) } else { ([0.; 3], 0.) })
    }
}

// ----- Every frame.

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hurt>().add_systems(Update, update.after(crate::arena::tick).before(crate::slowmo::present));
    }
}

/// Every frame after the round: what the records tell, the indicators and the marker kept, the slow motion told of
/// kills, and all of it drawn.
pub fn update(world: &mut World) {
    // (A replay watched through a player's recorded view goes by that player's clock: replay/pov.rs.)
    let now = crate::replay::pov::now(world);
    let settings = world.get_resource::<crate::settings::Settings>().copied().unwrap_or_default();
    let mut hurt = world.remove_resource::<Hurt>().unwrap_or_default();
    // What the round tells, and where this player is and looks.
    let mut giant_news = vec![];
    let (events, view, network) = match world.get_resource::<Game>() {
        Some(g) if g.playing() && !g.demo => {
            let events = hurt.watch.hear(g.me(), &g.roster, g.elapsed, g.phase_name());
            let me = g.roster.iter().find(|p| p.id == g.me());
            let alive = me.is_some_and(|p| p.alive) && !crate::cards::at_table(g);
            // (Where this player is drawn: online its prediction, ahead of the server's word.)
            let eye = g.position + Vec3::Y * me.map_or(feast::CHEST[0], |p| feast::CHEST[if p.prone { 2 } else if p.crouch { 1 } else { 0 }]);
            hurt.age(&g.roster, now);
            // Titan: this player the giant, its full health, and what its events tell that is new.
            let titan = g.titan.as_ref().filter(|v| v.titan != 0 && v.titan == g.me());
            hurt.giant = titan.map(|v| v.max);
            if let Some(v) = titan {
                giant_news = giant_heard(&mut hurt.told, v, &g.roster);
            }
            (events, alive.then_some((eye, g.yaw)), g.network)
        }
        Some(_) => {
            hurt.watch = Watch::default();
            (hurt.giant, hurt.told) = (None, 0);
            (vec![], None, false)
        }
        None => {
            world.insert_resource(hurt);
            return;
        }
    };
    // A replay watched through a player's recorded view hears what that player's screen heard, when it heard it.
    let events = crate::replay::pov::heard(world).unwrap_or(events);
    if view.is_none() {
        // Dead (or not in a round): nothing of first person, and what was shown is forgotten.
        (hurt.arcs, hurt.ring, hurt.marker) = (vec![], None, None);
    }
    hurt.heard.clone_from(&events);
    for e in &events {
        match *e {
            Event::Hurt(record) => {
                if let (Some((eye, _)), true) = (view, settings.damage_indicators) {
                    match hurt.giant {
                        Some(max) => hurt.giant_hit(&record, eye, now, max),
                        None => hurt.hit(&record, eye, now),
                    }
                }
            }
            Event::Dealt { kill, at, .. } => {
                hurt.mark(now, kill);
                if kill {
                    world.resource_mut::<Game>().mark_hit(KILL);
                    if settings.kill_slowmo {
                        if let Some(mut s) = world.get_resource_mut::<crate::slowmo::SlowMo>() {
                            s.kill(now, network, Some(at));
                        }
                    }
                }
            }
            Event::Kills => {
                hurt.mark(now, true);
                world.resource_mut::<Game>().mark_hit(KILL);
            }
            Event::Final { at, .. } => {
                if settings.kill_slowmo {
                    if let Some(mut s) = world.get_resource_mut::<crate::slowmo::SlowMo>() {
                        s.kill(now, true, Some(at));
                    }
                }
            }
        }
    }
    if let (Some((eye, _)), true) = (view, settings.damage_indicators) {
        for news in giant_news {
            match news {
                GiantNews::Weak { by, from, amount } => hurt.weak_hit(by, from, eye, now, amount),
                GiantNews::Stagger => hurt.stagger(now),
            }
        }
    }
    if view.is_none() {
        hurt.marker = None;
    }
    if !settings.damage_indicators {
        (hurt.arcs, hurt.ring, hurt.pulse) = (vec![], None, None);
    }
    draw(world, &mut hurt, view, now);
    world.insert_resource(hurt);
}

/// Titan: what the giant's events tell its own player that a record does not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GiantNews {
    /// A weak-spot hit: by whom, where it was (its chest), and the health it took.
    Weak { by: u32, from: Vec3, amount: f32 },
    /// The giant staggers.
    Stagger,
}
/// Titan: the giant's events in `view` newer than `told` (which it moves on; numbered afresh each round, so a count
/// that went back starts again), as the giant's player is told them; the shooters' places from `roster`.
pub fn giant_heard(told: &mut u32, view: &kitchen_sim::titan::View, roster: &[Player]) -> Vec<GiantNews> {
    use kitchen_sim::titan::EventKind;
    let newest = view.events.iter().map(|e| e.id).max().unwrap_or(0);
    if newest < *told {
        *told = 0;
    }
    let mut out = vec![];
    for e in view.events.iter().filter(|e| e.id > *told) {
        match e.kind {
            Some(EventKind::WeakHit) if e.to == view.titan || e.to == 0 => {
                let from = roster.iter().find(|p| p.id == e.by).map_or(Vec3::from_array(e.p), chest);
                out.push(GiantNews::Weak { by: e.by, from, amount: e.amount });
            }
            Some(EventKind::StaggerStart) => out.push(GiantNews::Stagger),
            _ => {}
        }
    }
    *told = (*told).max(newest);
    out
}

// ----- On the screen.

/// The nodes: a pool of arcs, the ring, the marker, the edges; built for one size.
struct Ui {
    root: Entity,
    arcs: Vec<Entity>,
    ring: Entity,
    marker: Entity,
    edges: Entity,
    /// The window's logical height share (hud.rs `Size`), scale factor and the giant's ring (logical pixels) it was
    /// built for.
    size: (f32, f32, f32),
    /// The images: the narrow arc, the wide, the ring, the hit's ticks, the kill's; and the side (logical pixels) of the
    /// ring's and of each marker's.
    images: [Handle<Image>; 5],
    sides: [f32; 3],
    /// Titan, the giant's own: a pool of cues on the wide ring, their images (soft, weak-spot) and sizes (logical
    /// pixels), how far out the middle of each image stands, and the weak spot's icon.
    giant: Vec<Entity>,
    giant_images: [Handle<Image>; 2],
    giant_sizes: [Vec2; 2],
    giant_mid: [f32; 2],
    weak_icon: Entity,
}

/// Device pixels a unit at SCALE takes.
fn unit(s: f32, sf: f32) -> f32 {
    s * sf
}
/// The rim's width in device pixels.
fn rim(u: f32) -> f32 {
    (2. * u).clamp(1.5, 5.)
}
/// The side in device pixels of the square the ring's shapes are drawn in.
fn ring_side(u: f32) -> u32 {
    (2. * (RADIUS + PEAK + 2.) * u + 2. * rim(u) + 2.).ceil() as u32
}
fn marker_side(u: f32, kill: bool) -> u32 {
    (2. * (TICKS[kill as usize].1 + 3.) * u + 2. * rim(u) + 2.).ceil() as u32
}
/// Titan: an image of just the arc at the top of a ring `radius` units out (`spread`, `thick` as [`arc_distance`]):
/// its size in device pixels, how far from the ring's middle its own middle is (device pixels), and the shape.
pub fn giant_arc(u: f32, radius: f32, spread: f32, thick: f32) -> (UVec2, f32, Vec<u8>) {
    let r = rim(u);
    let (outer, inner) = ((radius + PEAK) * u, (radius - thick) * u * spread.cos());
    let mid = (outer + inner) / 2.;
    let half = Vec2::new(outer * spread.sin() + thick * u + r + 2., (outer - inner) / 2. + r + 2.);
    let size = (half * 2.).ceil().as_uvec2();
    // (Drawn about the image's middle, which stands `mid` out from the ring's.)
    let (w, h) = (size.x as f32, size.y as f32);
    let mut data = vec![0u8; (size.x * size.y * 4) as usize];
    let side = size.x.max(size.y);
    let square = raster(side, r, |p| arc_distance_on(p - Vec2::new(0., mid), u, radius, spread, thick));
    let (ox, oy) = ((side as f32 - w) / 2., (side as f32 - h) / 2.);
    for y in 0..size.y {
        for x in 0..size.x {
            let (sx, sy) = ((x as f32 + ox) as u32, (y as f32 + oy) as u32);
            let (from, to) = (((sy * side + sx) * 4) as usize, ((y * size.x + x) * 4) as usize);
            data[to..to + 4].copy_from_slice(&square[from..from + 4]);
        }
    }
    (size, mid, data)
}

/// The signed distance (device pixels; negative inside) from `p` (pixels from the middle, y down) to an arc at the top of
/// the ring: `spread` radians either side of straight up, `thick` units thick in the middle and thinning to its ends,
/// its outer edge pointing out PEAK at its middle.
pub fn arc_distance(p: Vec2, u: f32, spread: f32, thick: f32) -> f32 {
    arc_distance_on(p, u, RADIUS, spread, thick)
}
/// As [`arc_distance`], on a ring `radius` units out.
pub fn arc_distance_on(p: Vec2, u: f32, radius: f32, spread: f32, thick: f32) -> f32 {
    let r = p.length();
    let phi = p.x.atan2(-p.y);
    let t = (phi.abs() / spread).min(1.);
    let out = (radius + PEAK * (1. - t)) * u;
    let inner = out - thick * (1. - t * t).sqrt().max(0.3) * u;
    let across = (inner - r).max(r - out);
    let along = (phi.abs() - spread) * r;
    if across > 0. && along > 0. { across.hypot(along) } else { across.max(along) }
}
fn ring_distance(p: Vec2, u: f32) -> f32 {
    let r = p.length();
    (((RADIUS - RING_THICK) * u) - r).max(r - RADIUS * u)
}
/// Four ticks on the diagonals, `from` to `to` units out and `wide` units wide.
fn ticks_distance(p: Vec2, u: f32, (from, to, wide): (f32, f32, f32)) -> f32 {
    (0..4)
        .map(|k| {
            let a = PI / 4. + k as f32 * PI / 2.;
            let dir = Vec2::new(a.cos(), a.sin());
            let (a, b) = (dir * from * u, dir * to * u);
            let along = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0., 1.);
            p.distance(a + (b - a) * along) - wide * u / 2.
        })
        .fold(f32::MAX, f32::min)
}
/// A shape `side` device pixels square from its signed distance (pixels from its middle): white inside, a dark rim
/// `rim` pixels wide round it, clear elsewhere, smoothed over a pixel; RGBA as sRGB, not premultiplied (icons.rs's look).
pub fn raster(side: u32, rim: f32, distance: impl Fn(Vec2) -> f32) -> Vec<u8> {
    let mut out = vec![0u8; (side * side * 4) as usize];
    let half = side as f32 / 2.;
    let srgb = |v: f32| {
        let v = v.clamp(0., 1.);
        if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 }
    };
    for y in 0..side {
        for x in 0..side {
            let d = distance(Vec2::new(x as f32 + 0.5 - half, y as f32 + 0.5 - half));
            let fill = (0.5 - d).clamp(0., 1.);
            let ink = ((0.5 - (d - rim)).clamp(0., 1.) - fill).max(0.) * INK_ALPHA;
            let alpha = fill + ink;
            if alpha <= 0. {
                continue;
            }
            let at = ((y * side + x) * 4) as usize;
            for k in 0..3 {
                out[at + k] = (srgb((fill + INK[k] * ink) / alpha) * 255.).round() as u8;
            }
            out[at + 3] = (alpha.min(1.) * 255.).round() as u8;
        }
    }
    out
}
fn image(images: &mut Assets<Image>, side: u32, data: Vec<u8>) -> Handle<Image> {
    images.add(Image::new(Extent3d { width: side, height: side, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default()))
}

/// The nodes for a window `s` (its height over SCALE) and `sf` (its scale factor), the giant's ring `radius` logical
/// pixels out.
fn build(world: &mut World, s: f32, sf: f32, radius: f32) -> Ui {
    let u = unit(s, sf);
    let (big, small, large) = (ring_side(u), marker_side(u, false), marker_side(u, true));
    let r = rim(u);
    let shapes: [(u32, Box<dyn Fn(Vec2) -> f32>); 5] = [
        (big, Box::new(move |p| arc_distance(p, u, SPREAD[0], THICK[0]))),
        (big, Box::new(move |p| arc_distance(p, u, SPREAD[1], THICK[1]))),
        (big, Box::new(move |p| ring_distance(p, u))),
        (small, Box::new(move |p| ticks_distance(p, u, TICKS[0]))),
        (large, Box::new(move |p| ticks_distance(p, u, TICKS[1]))),
    ];
    let images = world.resource_scope(|_, mut assets: Mut<Assets<Image>>| shapes.map(|(side, shape)| image(&mut assets, side, raster(side, r, shape))));
    let root = world
        .spawn((
            Name::new("Damage indicators and hit marker"),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() },
            Visibility::Hidden,
            // (Over the scope's mask and the edges' darkening; under the head-up display's icons at the edges, which
            // these never reach, and the menu.)
            ZIndex(1),
        ))
        .id();
    let edges = world.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, Visibility::Hidden)).id();
    world.entity_mut(root).add_child(edges);
    let centred = |side: u32| {
        let w = side as f32 / sf;
        Node { position_type: PositionType::Absolute, left: Val::Percent(50.), top: Val::Percent(50.), width: Val::Px(w), height: Val::Px(w), margin: UiRect { left: Val::Px(-w / 2.), top: Val::Px(-w / 2.), ..default() }, ..default() }
    };
    let node = |world: &mut World, side: u32, image: &Handle<Image>| {
        let e = world.spawn((ImageNode::new(image.clone()), centred(side), UiTransform::default(), Visibility::Hidden)).id();
        world.entity_mut(root).add_child(e);
        e
    };
    let arcs = (0..ARCS).map(|_| node(world, big, &images[0])).collect();
    let ring = node(world, big, &images[2]);
    let marker = node(world, small, &images[3]);
    // The giant's: on a ring `radius` out (in units of SCALE pixels), soft and weak-spot.
    let units = radius / s.max(1e-3);
    let made = [0, 1].map(|k| giant_arc(u, units, GIANT_SPREAD[k], GIANT_THICK[k]));
    let giant_sizes = made.each_ref().map(|(size, ..)| size.as_vec2() / sf);
    let giant_mid = made.each_ref().map(|(_, mid, _)| mid / sf);
    let giant_images = world.resource_scope(|_, mut assets: Mut<Assets<Image>>| {
        made.map(|(size, _, data)| assets.add(Image::new(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())))
    });
    let giant = (0..ARCS)
        .map(|_| {
            let e = world
                .spawn((ImageNode::new(giant_images[0].clone()), Node { position_type: PositionType::Absolute, left: Val::Percent(50.), top: Val::Percent(50.), ..default() }, UiTransform::default(), Visibility::Hidden))
                .id();
            world.entity_mut(root).add_child(e);
            e
        })
        .collect();
    let icon_px = (18. * u).round() as u32;
    let icon = world.resource_scope(|world, mut icons: Mut<crate::icons::Icons>| icons.get(&mut world.resource_mut::<Assets<Image>>(), crate::icons::Icon::Weak, icon_px));
    let weak_icon = world
        .spawn((ImageNode::new(icon).with_color(rgba(WEAK_RGB, 1.)), Node { position_type: PositionType::Absolute, left: Val::Percent(50.), top: Val::Percent(50.), ..default() }, Visibility::Hidden))
        .id();
    world.entity_mut(root).add_child(weak_icon);
    Ui {
        root,
        arcs,
        ring,
        marker,
        edges,
        size: (s, sf, radius),
        images,
        sides: [big as f32 / sf, small as f32 / sf, large as f32 / sf],
        giant,
        giant_images,
        giant_sizes,
        giant_mid,
        weak_icon,
    }
}

fn rgba(c: [f32; 3], a: f32) -> Color {
    Color::srgba(c[0], c[1], c[2], a)
}
fn show(world: &mut World, e: Entity, on: bool) {
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if world.get::<Visibility>(e).is_some_and(|v| *v != want) {
        world.entity_mut(e).insert(want);
    }
}

/// Puts what `hurt` holds on the screen for a player at `view` (its chest and yaw; None: dead or not in a round).
fn draw(world: &mut World, hurt: &mut Hurt, view: Option<(Vec3, f32)>, now: f32) {
    let busy = !hurt.arcs.is_empty() || hurt.ring.is_some() || hurt.pulse.is_some() || hurt.marker.is_some();
    if view.is_none() || !busy {
        if let Some(ui) = &hurt.ui {
            let root = ui.root;
            show(world, root, false);
        }
        return;
    }
    let (eye, yaw) = view.unwrap();
    let (s, sf, radius) = world
        .query::<&Window>()
        .iter(world)
        .next()
        .map_or((1., 1., giant_radius(1280., 720.)), |w| ((w.height() / crate::hud::SCALE).clamp(0.8, 4.), w.scale_factor(), giant_radius(w.width(), w.height())));
    if hurt.ui.as_ref().is_none_or(|ui| (ui.size.0 - s).abs() > 1e-3 || (ui.size.1 - sf).abs() > 1e-3 || (ui.size.2 - radius).abs() > 0.5 || world.get_entity(ui.root).is_err()) {
        if let Some(old) = hurt.ui.take() {
            if let Ok(e) = world.get_entity_mut(old.root) {
                e.despawn();
            }
        }
        if !world.contains_resource::<crate::icons::Icons>() {
            world.init_resource::<crate::icons::Icons>();
        }
        hurt.ui = Some(build(world, s, sf, radius));
    }
    let ui = hurt.ui.as_ref().unwrap();
    show(world, ui.root, true);
    // The arcs, newest last (drawn on top): round the crosshair, or the giant's out on its wide ring.
    let mut arcs: Vec<&Arc> = hurt.arcs.iter().collect();
    arcs.sort_by(|a, b| a.born.total_cmp(&b.born));
    let giant = hurt.giant.is_some();
    let mut icon = None;
    for (k, &e) in ui.giant.iter().enumerate() {
        let Some(arc) = arcs.get(k).filter(|_| giant) else {
            show(world, e, false);
            continue;
        };
        let kind = arc.weak as usize;
        let (size, mid, theta) = (ui.giant_sizes[kind], ui.giant_mid[kind], arc.angle(eye, yaw));
        let image = ui.giant_images[kind].clone();
        if let Some(mut node) = world.get_mut::<ImageNode>(e) {
            node.color = rgba(arc.tint, arc.giant_alpha(now));
            if node.image != image {
                node.image = image;
            }
        }
        let at = Vec2::new(theta.sin(), -theta.cos()) * mid;
        if let Some(mut node) = world.get_mut::<Node>(e) {
            let margin = UiRect { left: Val::Px(at.x - size.x / 2.), top: Val::Px(at.y - size.y / 2.), ..default() };
            if node.margin != margin || node.width != Val::Px(size.x) {
                (node.width, node.height, node.margin) = (Val::Px(size.x), Val::Px(size.y), margin);
            }
        }
        if let Some(mut t) = world.get_mut::<UiTransform>(e) {
            t.rotation = Rot2::radians(theta);
            t.scale = Vec2::splat(if arc.weak { pop(now - arc.popped).powi(2) } else { 1. });
        }
        show(world, e, true);
        if arc.weak {
            icon = Some((theta, mid, arc.giant_alpha(now)));
        }
    }
    // The weak spot's icon just inside the newest weak-spot hit's cue.
    if let Some((theta, mid, alpha)) = icon {
        let side = 18. * s;
        let at = Vec2::new(theta.sin(), -theta.cos()) * (mid - side * 1.3);
        if let Some(mut node) = world.get_mut::<Node>(ui.weak_icon) {
            (node.width, node.height, node.margin) = (Val::Px(side), Val::Px(side), UiRect { left: Val::Px(at.x - side / 2.), top: Val::Px(at.y - side / 2.), ..default() });
        }
        if let Some(mut node) = world.get_mut::<ImageNode>(ui.weak_icon) {
            node.color = rgba(WEAK_RGB, alpha);
        }
    }
    show(world, ui.weak_icon, icon.is_some());
    for (k, &e) in ui.arcs.iter().enumerate() {
        let Some(arc) = arcs.get(k).filter(|_| !giant) else {
            show(world, e, false);
            continue;
        };
        let image = ui.images[arc.wide as usize].clone();
        if let Some(mut node) = world.get_mut::<ImageNode>(e) {
            node.color = rgba(arc.tint, arc.alpha(now));
            if node.image != image {
                node.image = image;
            }
        }
        if let Some(mut t) = world.get_mut::<UiTransform>(e) {
            t.rotation = Rot2::radians(arc.angle(eye, yaw));
            t.scale = Vec2::splat(pop(now - arc.popped) * (0.97 + 0.06 * arc.weight()));
        }
        show(world, e, true);
    }
    // The ring's pulse.
    let ring = hurt.ring.map(|r| (r, now - r.born));
    if let Some((r, age)) = ring {
        if let Some(mut node) = world.get_mut::<ImageNode>(ui.ring) {
            node.color = rgba(r.tint, ring_alpha(age));
        }
        if let Some(mut t) = world.get_mut::<UiTransform>(ui.ring) {
            t.scale = Vec2::splat(ring_scale(age));
        }
    }
    show(world, ui.ring, ring.is_some());
    // The hit marker.
    if let Some((at, kill)) = hurt.marker {
        let age = now - at;
        let life = if kill { KILL } else { HIT };
        let alpha = 1. - smooth((age / life - 0.4) / 0.6);
        let pop = if kill { 1. + 0.55 * (-age / 0.06).exp() } else { 1. + 0.25 * (-age / 0.05).exp() };
        let side = ui.sides[1 + kill as usize];
        let image = ui.images[3 + kill as usize].clone();
        if let Some(mut node) = world.get_mut::<ImageNode>(ui.marker) {
            node.color = rgba(if kill { KILL_RGB } else { MARK_RGB }, alpha);
            if node.image != image {
                node.image = image;
            }
        }
        if let Some(mut node) = world.get_mut::<Node>(ui.marker) {
            if node.width != Val::Px(side) {
                (node.width, node.height, node.margin) = (Val::Px(side), Val::Px(side), UiRect { left: Val::Px(-side / 2.), top: Val::Px(-side / 2.), ..default() });
            }
        }
        if let Some(mut t) = world.get_mut::<UiTransform>(ui.marker) {
            t.scale = Vec2::splat(pop);
        }
    }
    show(world, ui.marker, hurt.marker.is_some());
    // The edges: a soft gradient in from each, as bright as the hits on that side.
    let edges = hurt.edges(eye, yaw, now);
    let lit = edges.iter().any(|(_, a)| *a > 0.01);
    if lit {
        let angles = [LinearGradient::TO_BOTTOM, LinearGradient::TO_LEFT, LinearGradient::TO_TOP, LinearGradient::TO_RIGHT];
        let gradients: Vec<Gradient> = edges
            .iter()
            .zip(angles)
            .enumerate()
            .filter(|(_, ((_, a), _))| *a > 0.01)
            .map(|(side, ((c, a), angle))| {
                // (The top and the bottom reach a little further in, of the screen's smaller height.)
                let reach = EDGE_IN[(side % 2 == 0) as usize];
                LinearGradient::new(angle, vec![ColorStop::percent(rgba(*c, *a), 0.), ColorStop::percent(rgba(*c, *a * 0.35), reach * 0.4), ColorStop::percent(rgba(*c, 0.), reach)]).into()
            })
            .collect();
        world.entity_mut(ui.edges).insert(BackgroundGradient(gradients));
    }
    show(world, ui.edges, lit);
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYE: Vec3 = Vec3::new(0., 1., 3.);
    fn hotdog(id: u32) -> Player {
        Player { id, p: [0., 0.9025, 3.], alive: true, hp: 100., ..default() }
    }
    fn shot(from: Vec3, attacker: u32, time: f32, amount: f32) -> Damage {
        Damage { kind: 1, attacker, p: from.to_array(), d: (EYE - from).normalize().to_array(), time, amount }
    }

    /// Looking along yaw 0 a hotdog faces -z: a shot from ahead stands at the top of the ring, from behind at the
    /// bottom, from its right (+x) at the right and its left at the left; turned, the same source moves round the
    /// other way.
    #[test]
    fn an_arc_points_at_its_source_ahead_behind_left_and_right_and_turns_with_the_view() {
        let close = |a: f32, b: f32| ((a - b + PI).rem_euclid(TAU) - PI).abs() < 1e-4;
        let ways = [(Vec3::NEG_Z, 0.), (Vec3::Z, PI), (Vec3::X, PI / 2.), (Vec3::NEG_X, -PI / 2.)];
        for (way, expected) in ways {
            let at = EYE + way * 0.6;
            assert!(close(angle(EYE, 0., at), expected), "{way}: {}", angle(EYE, 0., at));
            // As an arc on the ring, from a hit's record.
            let mut hurt = Hurt::default();
            hurt.hit(&shot(at, 2, 1., 20.), EYE, 0.);
            assert!(close(hurt.arcs[0].angle(EYE, 0.), expected));
            // Turned a quarter to the left (yaw +pi/2 looks along -x), the source is a quarter further clockwise.
            assert!(close(hurt.arcs[0].angle(EYE, PI / 2.), expected + PI / 2.), "{way} turned");
            assert!(close(hurt.arcs[0].angle(EYE, PI), expected + PI), "{way} turned round");
        }
        // The arc follows the hotdog that shot while it lives.
        let mut hurt = Hurt::default();
        hurt.hit(&shot(EYE + Vec3::NEG_Z * 0.6, 2, 1., 20.), EYE, 0.);
        let mut them = hotdog(2);
        them.p = [0.5, 0.9025, 3.];
        hurt.age(&[hotdog(1), them], 0.1);
        assert!(close(hurt.arcs[0].angle(EYE, 0.), PI / 2.), "it walked to the right");
    }

    /// Hunger, a fall and a hit from straight above have no way to point: the whole ring pulses, faintly. A hot griddle
    /// is from below, a blow is wide.
    #[test]
    fn a_hit_with_no_way_to_point_pulses_the_whole_ring() {
        let hunger = Damage { kind: feast::HUNGER, p: EYE.to_array(), time: 1., amount: 2., ..default() };
        let fall = Damage { kind: feast::FALL, p: EYE.to_array(), d: [0., -1., 0.], time: 1., amount: 100., ..default() };
        let above = Damage { kind: 1, attacker: 2, p: (EYE + Vec3::new(0.05, 0.8, 0.)).to_array(), d: [0., -1., 0.], time: 1., amount: 10. };
        let below = Damage { kind: 1, attacker: 2, p: (EYE + Vec3::new(0.0, -0.6, 0.1)).to_array(), d: [0., 1., 0.], time: 1., amount: 10. };
        let swarm = Damage { kind: feast::PESTS, p: EYE.to_array(), time: 1., amount: 1., ..default() };
        for record in [hunger, fall, above, below, swarm] {
            assert_eq!(cue(&record, EYE), Cue::Around, "{record:?}");
            let mut hurt = Hurt::default();
            hurt.hit(&record, EYE, 5.);
            assert!(hurt.arcs.is_empty() && hurt.ring.is_some(), "{record:?}");
        }
        assert!(ring_alpha(0.) > 0. && ring_alpha(0.) <= 0.6, "faint");
        assert_eq!(ring_alpha(RING_LIFE), 0.);
        assert!(ring_scale(0.5) > ring_scale(0.), "it spreads");
        let burn = Damage { kind: feast::BURN, p: EYE.to_array(), time: 1., amount: 1., ..default() };
        assert_eq!(cue(&burn, EYE), Cue::Below);
        let mut hurt = Hurt::default();
        hurt.hit(&burn, EYE, 0.);
        assert!(hurt.arcs[0].below && (hurt.arcs[0].angle(EYE, 1.3) - PI).abs() < 1e-6 && hurt.arcs[0].tint == BURN_RGB);
        let blow = Damage { kind: feast::MELEE, attacker: 2, p: (EYE + Vec3::X * 0.1).to_array(), time: 1., amount: 50., ..default() };
        assert!(matches!(cue(&blow, EYE), Cue::From { wide: true, follow: 2, .. }));
        assert!(matches!(cue(&shot(EYE + Vec3::X, 2, 1., 10.), EYE), Cue::From { wide: false, .. }));
        // A glob bursting right on the hotdog points back the way it came.
        let glob = Damage { kind: 4, attacker: 2, p: EYE.to_array(), d: [1., 0., 0.], time: 1., amount: 30. };
        match cue(&glob, EYE) {
            Cue::From { at, .. } => assert!((angle(EYE, 0., at) + PI / 2.).abs() < 1e-4, "from the left"),
            other => panic!("{other:?}"),
        }
    }

    /// Whole for WHOLE seconds, then fading, gone at LIFE (and forgotten); the edge's flash is brief.
    #[test]
    fn an_arc_is_whole_for_a_moment_and_fades_out_by_its_life() {
        assert_eq!(fade(0.), 1.);
        assert_eq!(fade(WHOLE), 1.);
        let mut last = 1.;
        for k in 1..=20 {
            let a = fade(WHOLE + (LIFE - WHOLE) * k as f32 / 20.);
            assert!(a <= last && a >= 0., "{k}");
            last = a;
        }
        assert!(fade(0.7) > 0. && fade(0.7) < 1.);
        assert_eq!(fade(LIFE), 0.);
        let mut hurt = Hurt::default();
        hurt.hit(&shot(EYE + Vec3::X, 2, 1., 30.), EYE, 10.);
        hurt.age(&[], 10. + LIFE - 0.01);
        assert_eq!(hurt.arcs.len(), 1);
        hurt.age(&[], 10. + LIFE + 0.01);
        assert!(hurt.arcs.is_empty());
        assert!(flash(0.) > flash(0.2) && flash(FLASH) == 0.);
        // More damage, brighter; two sources, two arcs.
        let mut hurt = Hurt::default();
        hurt.hit(&shot(EYE + Vec3::X, 2, 1., 5.), EYE, 0.);
        hurt.hit(&shot(EYE + Vec3::NEG_X, 3, 1., 60.), EYE, 0.);
        assert_eq!(hurt.arcs.len(), 2);
        assert!(hurt.arcs[1].alpha(0.) > hurt.arcs[0].alpha(0.));
        // Two at once flash two edges, the right and the left (the heavier brighter), and only for a moment.
        let edges = hurt.edges(EYE, 0., 0.1);
        assert!(edges[1].1 > 0.05 && edges[3].1 > edges[1].1 && edges[0].1 < 1e-3 && edges[2].1 < 1e-3, "{edges:?}");
        assert!(hurt.edges(EYE, 0., FLASH).iter().all(|(_, a)| *a == 0.));
        // The same source again: one arc, refreshed, its damage added up.
        hurt.hit(&shot(EYE + Vec3::X, 2, 1.5, 5.), EYE, 0.5);
        assert_eq!(hurt.arcs.len(), 2);
        assert!(hurt.arcs[0].born == 0.5 && hurt.arcs[0].hurt > 5.);
    }

    /// The records tell: a hit on this player, a hit of this player's, its kill, a cat it killed, and the death that
    /// ended the round. Seen for the first time, a record tells nothing; an old one is not news.
    #[test]
    fn the_records_tell_a_hit_taken_a_hit_dealt_and_a_kill() {
        let mut watch = Watch::default();
        let mut roster = vec![hotdog(1), hotdog(2), hotdog(3)];
        roster[0].last_damage = shot(Vec3::ZERO, 2, 0.5, 10.);
        assert!(watch.hear(1, &roster, 1., "FreeForAll").is_empty(), "first seen: nothing");
        roster[0].last_damage = shot(Vec3::X, 2, 1.1, 10.);
        roster[1].last_damage = shot(Vec3::X, 1, 1.1, 25.);
        let heard = watch.hear(1, &roster, 1.1, "FreeForAll");
        assert!(heard.contains(&Event::Hurt(roster[0].last_damage)));
        assert!(heard.iter().any(|e| matches!(e, Event::Dealt { victim: 2, kill: false, .. })));
        // The same records again: nothing new.
        assert!(watch.hear(1, &roster, 1.2, "FreeForAll").is_empty());
        // Hotdog 3 killed by this player; its kills go up with it (one kill, not two).
        roster[2].last_damage = shot(Vec3::X, 1, 1.3, 100.);
        roster[2].alive = false;
        roster[0].kills = 1;
        let heard = watch.hear(1, &roster, 1.3, "FreeForAll");
        assert_eq!(heard.len(), 1);
        assert!(matches!(heard[0], Event::Dealt { victim: 3, kill: true, .. }));
        // A cat: the kills go up with no hotdog's death.
        roster[0].kills = 2;
        assert_eq!(watch.hear(1, &roster, 1.4, "FreeForAll"), vec![Event::Kills]);
        // Another's kill is not this player's; a stale record is no news.
        roster[1].last_damage = shot(Vec3::X, 3, 1.5, 100.);
        roster[1].alive = false;
        assert!(watch.hear(1, &roster, 1.5, "FreeForAll").is_empty());
        roster[0].last_damage = shot(Vec3::X, 3, -5., 10.);
        assert!(watch.hear(1, &roster, 1.6, "FreeForAll").is_empty(), "stale");
        // The round's last death.
        roster[1].alive = true;
        watch.hear(1, &roster, 1.7, "FreeForAll");
        roster[1].alive = false;
        let heard = watch.hear(1, &roster, 1.8, "RoundOver");
        assert!(heard.iter().any(|e| matches!(e, Event::Final { victim: 2, .. })), "{heard:?}");
        // The marker: a hit, then a kill, which a hit does not cut short.
        let mut hurt = Hurt::default();
        hurt.mark(0., false);
        assert_eq!(hurt.marker, Some((0., false)));
        hurt.mark(0.1, true);
        hurt.mark(0.2, false);
        assert_eq!(hurt.marker, Some((0.1, true)));
        hurt.age(&[], 0.1 + KILL);
        assert_eq!(hurt.marker, None);
    }

    #[test]
    fn the_giant_sees_one_soft_cue_for_each_way_its_hits_come_from_as_heavy_as_its_share_and_no_red_edges() {
        let max = 41_000.;
        let mut hurt = Hurt { giant: Some(max), ..default() };
        // Five small hotdogs round its feet, all ahead of it, each firing: one cue, not five, and the edges stay clear.
        for (k, id) in (2..7).enumerate() {
            let at = EYE + Vec3::new(-0.2 + 0.1 * k as f32, 0., -1.2);
            hurt.giant_hit(&shot(at, id, 1., 36.), EYE, 0.01 * k as f32, max);
        }
        assert_eq!(hurt.arcs.len(), 1, "{:?}", hurt.arcs.iter().map(|a| a.key).collect::<Vec<_>>());
        assert!(hurt.edges(EYE, 0., 0.05).iter().all(|(_, a)| *a == 0.), "no flash for ordinary rounds");
        // Five rounds of a pistol are a sliver of 41,000: a faint cue. A hotdog's for the same is far stronger.
        let soft = &hurt.arcs[0];
        assert!(soft.giant_alpha(0.05) < 0.3 && soft.wide && !soft.weak, "{}", soft.giant_alpha(0.05));
        let mut small = Hurt::default();
        for id in 2..7 {
            small.hit(&shot(EYE + Vec3::NEG_Z, id, 1., 36.), EYE, 0.);
        }
        assert!(small.arcs.len() == 5 && small.edges(EYE, 0., 0.05).iter().any(|(_, a)| *a > 0.2), "a hotdog's feedback is as it was");
        // A heavy share (3% of its health within a moment) is a full cue; from behind it is a second one.
        hurt.giant_hit(&shot(EYE + Vec3::new(0., 0., -1.2), 2, 1., max * GIANT_SHARE), EYE, 0.06, max);
        assert!((hurt.arcs[0].weight() - 1.).abs() < 1e-3);
        hurt.giant_hit(&shot(EYE + Vec3::Z * 1.5, 3, 1., 36.), EYE, 0.07, max);
        assert_eq!(hurt.arcs.len(), 2);
        // From every way round at once: never more cues than ARCS, none of them following anyone.
        for k in 0..16 {
            let a = k as f32 * TAU / 16.;
            hurt.giant_hit(&shot(EYE + Vec3::new(a.sin(), 0., a.cos()) * 1.5, 2 + k, 1., 36.), EYE, 0.1, max);
        }
        assert!(hurt.arcs.len() <= ARCS && hurt.arcs.iter().all(|a| a.follow == 0 && !a.weak));
        // A fall has no way to point: nothing, and no ring over the crosshair.
        hurt.giant_hit(&Damage { kind: feast::FALL, amount: 500., time: 1., ..default() }, EYE, 0.2, max);
        assert!(hurt.ring.is_none());
    }

    #[test]
    fn a_weak_spot_hit_on_the_giant_is_gold_points_at_the_shooter_and_flashes_that_edge_and_a_stagger_pulses_them_all() {
        let mut hurt = Hurt { giant: Some(41_000.), ..default() };
        let behind = EYE + Vec3::Z * 1.4;
        hurt.weak_hit(4, behind, EYE, 0., 300.);
        let arc = &hurt.arcs[0];
        assert!(arc.weak && arc.tint == WEAK_RGB && arc.follow == 4 && arc.giant_alpha(0.) == 1.);
        assert!((arc.angle(EYE, 0.).abs() - PI).abs() < 1e-3, "behind is the bottom of the screen");
        // Its edge (the bottom: behind) flashes gold; the others hardly.
        let [top, right, bottom, left] = hurt.edges(EYE, 0., 0.05);
        assert!(bottom.1 > 0.2 && bottom.0 == WEAK_RGB, "{bottom:?}");
        assert!(top.1 < 0.01 && right.1 < 0.01 && left.1 < 0.01);
        // The shooter moves: its cue follows it.
        let mut shooter = hotdog(4);
        shooter.p = [EYE.x + 1.4, 0.9025, EYE.z];
        hurt.age(&[shooter], 0.1);
        assert!((hurt.arcs[0].angle(EYE, 0.) - PI / 2.).abs() < 0.1, "now to the right");
        // A stagger: every edge, once, amber.
        let mut calm = Hurt { giant: Some(41_000.), ..default() };
        calm.stagger(1.);
        let edges = calm.edges(EYE, 0., 1.05);
        assert!(edges.iter().all(|(c, a)| *a > 0.2 && *c == STAGGER_RGB));
        calm.age(&[], 1. + FLASH);
        assert!(calm.pulse.is_none() && calm.edges(EYE, 0., 1. + FLASH).iter().all(|(_, a)| *a == 0.));
    }

    #[test]
    fn the_giant_hears_each_weak_spot_hit_and_stagger_once_and_again_next_round() {
        use kitchen_sim::titan::{Event as TitanEvent, EventKind, View};
        let mut view = View { titan: 1, max: 41_000., ..default() };
        let shooter = Player { id: 3, p: [1., 0.9025, 2.], alive: true, ..default() };
        view.events = vec![
            TitanEvent { id: 1, kind: Some(EventKind::Hit), by: 3, to: 1, amount: 36., ..default() },
            TitanEvent { id: 2, kind: Some(EventKind::WeakHit), by: 3, to: 1, amount: 120., ..default() },
            TitanEvent { id: 3, kind: Some(EventKind::StaggerStart), ..default() },
        ];
        let mut told = 0;
        let news = giant_heard(&mut told, &view, std::slice::from_ref(&shooter));
        assert_eq!(news, vec![GiantNews::Weak { by: 3, from: chest(&shooter), amount: 120. }, GiantNews::Stagger]);
        assert!(giant_heard(&mut told, &view, &[]).is_empty(), "once");
        // A new round numbers them afresh.
        view.events = vec![TitanEvent { id: 1, kind: Some(EventKind::WeakHit), by: 3, to: 1, amount: 50., ..default() }];
        assert_eq!(giant_heard(&mut told, &view, &[]).len(), 1);
    }

    #[test]
    fn the_giants_cues_stand_clear_of_the_crosshair_and_of_the_giant_in_its_third_person() {
        // On a 1280 by 720 window the ring is 259 pixels out; a cue's inside edge is well clear of the crosshair's ring
        // and of the giant, whom its third person frames within about 140 pixels of the middle across and 190 down.
        let radius = giant_radius(1280., 720.);
        assert!((radius - 259.2).abs() < 0.1);
        for k in 0..2 {
            let (size, mid, data) = giant_arc(1., radius, GIANT_SPREAD[k], GIANT_THICK[k]);
            assert_eq!(data.len(), (size.x * size.y * 4) as usize);
            let inner = mid - size.y as f32 / 2.;
            assert!(inner > RADIUS * 2.5 && inner > 200., "{inner}");
            assert!(size.x < 240 && size.y < 60, "a cue, not a ring: {size}");
            // Its middle row is drawn (the arc is in the image).
            let row = (size.y / 2) as usize;
            assert!((0..size.x as usize).any(|x| data[(row * size.x as usize + x) * 4 + 3] > 200));
        }
        // On a phone held upright, inside the narrow side.
        assert!(giant_radius(390., 844.) + 30. < 195.);
    }

    /// The shapes: an arc is drawn at the top of its square and nowhere else, white with a dark rim; the marker's ticks
    /// leave the crosshair clear.
    #[test]
    fn the_arc_is_drawn_at_the_top_of_the_ring_and_the_ticks_leave_the_middle_clear() {
        let u = 1.;
        let side = ring_side(u);
        let data = raster(side, rim(u), |p| arc_distance(p, u, SPREAD[0], THICK[0]));
        let at = |x: f32, y: f32| {
            let (x, y) = ((side as f32 / 2. + x) as u32, (side as f32 / 2. + y) as u32);
            let k = ((y * side + x) * 4) as usize;
            [data[k], data[k + 1], data[k + 2], data[k + 3]]
        };
        assert_eq!(at(0., -(RADIUS - 3.)), [255, 255, 255, 255], "white in the middle of the arc");
        assert_eq!(at(0., RADIUS - 3.)[3], 0, "nothing at the bottom");
        assert_eq!(at(0., 0.)[3], 0, "nothing in the middle");
        let edge = at(0., -(RADIUS + PEAK + 1.));
        assert!(edge[3] > 100 && edge[0] < 60, "a dark rim: {edge:?}");
        let ticks = raster(marker_side(u, false), rim(u), |p| ticks_distance(p, u, TICKS[0]));
        let middle = (marker_side(u, false) / 2 * marker_side(u, false) + marker_side(u, false) / 2) as usize * 4;
        assert_eq!(ticks[middle + 3], 0, "the crosshair is clear");
    }

    // ----- In the game.

    use crate::arena::test_frame;
    use crate::slowmo::{SlowMo, SLOW};

    /// Single player (a siege's warm-up: nobody shoots anybody), with this module and the slow motion as `main` has them.
    fn offline(settings: crate::settings::Settings) -> App {
        let mut app = crate::arena::test_app("");
        app.insert_resource(settings).init_resource::<Hurt>().init_resource::<SlowMo>().add_systems(Update, (crate::arena::tick, update, crate::slowmo::present).chain());
        for _ in 0..3 {
            test_frame(&mut app, 0.05);
        }
        app
    }
    /// Hotdog `victim` hurt by this player's mustard (`kill`: killed by it), on the round's own record.
    fn struck(app: &mut App, victim: u32, kill: bool) {
        let mut g = app.world_mut().resource_mut::<Game>();
        let time = g.feast.elapsed;
        let p = g.feast.player_mut(victim).unwrap();
        let amount = if kill { p.hp } else { 20. };
        p.last_damage = Damage { kind: 1, attacker: 1, p: [0., 1., 3.], d: [0., 0., -1.], time, amount };
        p.hp -= amount;
        if kill {
            (p.alive, p.respawn) = (false, 5.);
        }
    }
    fn marker(app: &App) -> Option<bool> {
        app.world().resource::<Hurt>().marker.map(|(_, kill)| kill)
    }

    /// A hit of this player's puts the marker up; a kill makes it the kill's, and turns the crosshair into a red cross;
    /// both go.
    #[test]
    fn the_hit_marker_shows_for_a_hit_dealt_and_a_kill_shows_the_kills() {
        let mut app = offline(default());
        assert_eq!(marker(&app), None);
        struck(&mut app, 2, false);
        test_frame(&mut app, 0.05);
        assert_eq!(marker(&app), Some(false));
        assert!(!app.world().resource::<Game>().hit_marked(), "a hit is not a kill");
        let shown = |app: &mut App| app.world_mut().query::<(&ImageNode, &Visibility)>().iter(app.world()).filter(|(_, v)| **v != Visibility::Hidden).count();
        assert!(shown(&mut app) >= 1, "the ticks are on the screen");
        for _ in 0..8 {
            test_frame(&mut app, 0.05);
        }
        assert_eq!(marker(&app), None, "gone after {HIT} s");
        struck(&mut app, 3, true);
        test_frame(&mut app, 0.05);
        assert_eq!(marker(&app), Some(true));
        assert!(app.world().resource::<Game>().hit_marked(), "the crosshair is a red cross");
        for _ in 0..16 {
            test_frame(&mut app, 0.05);
        }
        assert_eq!(marker(&app), None);
    }

    /// Offline a kill of this player's slows the round itself to a quarter, and it comes back; with the setting off,
    /// it never slows.
    #[test]
    fn offline_a_kill_slows_the_round_itself_and_it_comes_back() {
        for on in [true, false] {
            let mut app = offline(crate::settings::Settings { kill_slowmo: on, ..default() });
            let step = |app: &mut App| {
                let was = app.world().resource::<Game>().feast.elapsed;
                test_frame(app, 0.05);
                app.world().resource::<Game>().feast.elapsed - was
            };
            assert!((step(&mut app) - 0.05).abs() < 1e-5, "at its own pace");
            struck(&mut app, 2, true);
            test_frame(&mut app, 0.05);
            assert_eq!(app.world().resource::<SlowMo>().start.is_some(), on);
            step(&mut app);
            let slowed = step(&mut app);
            let expected = if on { 0.05 * SLOW } else { 0.05 };
            assert!((slowed - expected).abs() < 1e-5, "on {on}: {slowed}");
            // The hotdogs are animated on the slowed clock, which falls behind real time while it lasts.
            let lost = app.world().resource::<SlowMo>().lost;
            assert_eq!(lost > 0.02, on, "{lost}");
            for _ in 0..24 {
                step(&mut app);
            }
            assert!((step(&mut app) - 0.05).abs() < 1e-5, "back to its own pace");
        }
    }

    /// Online the slow motion is for the screen alone: the round's clock, the timeline the others are drawn from, this
    /// player's prediction and its commands go exactly as they would without it.
    #[test]
    fn online_the_slow_motion_never_touches_the_round_or_its_clock() {
        let run = |kill: bool| {
            let mut app = offline(default());
            {
                let mut g = app.world_mut().resource_mut::<Game>();
                (g.network, g.network_id) = (true, 1);
            }
            test_frame(&mut app, 0.05);
            if kill {
                let mut g = app.world_mut().resource_mut::<Game>();
                let time = g.elapsed;
                let p = g.roster.iter_mut().find(|p| p.id == 2).unwrap();
                p.last_damage = Damage { kind: 1, attacker: 1, p: [0., 1., 3.], d: [0., 0., -1.], time, amount: 100. };
                (p.hp, p.alive) = (0., false);
            }
            let (mut slowed, mut log) = (false, vec![]);
            for _ in 0..30 {
                test_frame(&mut app, 0.05);
                let now = app.world().resource::<Time<Real>>().elapsed_secs();
                slowed |= app.world().resource::<SlowMo>().running(now);
                assert_eq!(crate::slowmo::sim_scale(app.world()), 1.);
                let g = app.world().resource::<Game>();
                log.push((g.feast.elapsed, g.elapsed, g.online.timeline.clock, g.position, g.online.seq, g.online.unsent.len()));
            }
            (slowed, app.world().resource::<SlowMo>().shown_only, log)
        };
        let (slowed, shown_only, with) = run(true);
        let (still, _, without) = run(false);
        assert!(slowed && shown_only && !still, "it showed, for the screen alone");
        assert_eq!(with, without);
    }

}

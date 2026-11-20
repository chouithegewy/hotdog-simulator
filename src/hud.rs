// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The head-up display in first person: minimal, mostly icons (icons.rs) and numbers, for players who know the
//! buttons. Words are left for names, and for a moment in the middle of the screen of one to three words ("YOU
//! DIED", "ROUND WON"); how to play is the hints' to say (hints.rs, H, off to begin with).
//!
//! * **Bottom left** ([`Vitals`]): a heart, the health bar and its number; over them the abilities, each an icon that
//!   is bright when ready, dim with its seconds while it cools down, and lit with its seconds left while it lasts: the
//!   stim, and while they apply the stance (crouching, prone), the free for all's spawn shield, an infection and the
//!   famine's hunger.
//! * **Bottom right** ([`Arms`]): the gun in hand, the gun itself in silhouette, a bullet and its rounds over its full
//!   load (or infinity), and over them the other guns held, small. Behind the forcefield a lock is on the gun. With
//!   the hands full, a hand (or the bacon) instead.
//! * **Top middle** ([`Top`]): the score and the clock. Forts: each team's rounds as pips in its colour either side
//!   of the clock, and under them the objective, an icon with an arrow that points the way and the distance (the
//!   bacon; carrying it, home). The siege: the wave's clock or the cats left, the hotdogs alive and this player's
//!   kills. Free For All: the clock, this player's kills of the kills to win, and the leader's.
//! * **Top right**: the kill feed ([`Kill`]): who killed (an icon in the killer's colour: a hotdog, or a cat, a
//!   turret, flies), with what (the gun's silhouette, a strike, a bite), and the victim's name. Kept a few seconds.
//! * **By the crosshair**: what a key would do here and now ([`prompt`]), a key cap and an icon ([E] and the gun on
//!   the pad, [Q] and a hand at something loose), shown only while it applies; and the seconds until an empty
//!   pad's pickup is back.
//! * **The middle**: a moment ([`Moment`]): the round won or lost, a death and the seconds until the respawn, the
//!   wave, the famine, the forcefield dropping.
//! * **Bottom middle**, dead: whom the death camera watches, and the button for the next.
//! * **Kung Fu** ([`Fight`]) has no gun, no rounds and no stim: bottom right instead its style (the animal, ringed while
//!   a Toad's iron body is up) under the chi meter, which glows gold when it is full, with the special's key beside it;
//!   the guard bar under the health while the guard is down; the KOs as pips of the KOs to win at the top, with the
//!   clock and the leader; the combo's count right of the crosshair, popping with each hit of a chain; a small icon over
//!   the crosshair for a blow blocked, a parry, a guard broken (bright when it was this player's doing, red when it was
//!   done to it); and the style's name for a moment when it changes, the only word.
//!
//! Everything is sized from the window's height: SCALE is 720 pixels, and the icons are drawn for the pixels they
//! take (icons.rs), so they are crisp at any size. Nothing here decides anything: it reads the round as the client
//! has it ([`Show::of`] is pure), offline and online alike.
use crate::arena::{Game, Weapon};
use crate::icons::{Icon, Icons};
use bevy::prelude::*;
use kitchen_sim::feast::{self, Player};
use kitchen_sim::forts::{DROPPED, HELD};
use std::collections::HashMap;

/// The window height the sizes here are given for.
pub const SCALE: f32 = 720.;
/// How far over the window's foot the gun and its rounds sit: in a browser clear of the page's frame-rate line, which
/// is along the bottom right (index.html's #readout).
const ARMS_FOOT: f32 = if cfg!(target_arch = "wasm32") { 36. } else { 18. };
/// Seconds a kill stays in the feed, and how many are shown at once.
pub const FEED_TIME: f32 = 5.;
pub const FEED_MAX: usize = 4;
/// The display's colours.
const WHITE: [f32; 3] = [0.96, 0.97, 0.93];
const DIM: [f32; 3] = [0.62, 0.68, 0.66];
const HEART: [f32; 3] = [0.96, 0.27, 0.27];
const MINT: [f32; 3] = [0.55, 0.96, 0.76];
const AMBER: [f32; 3] = [1.0, 0.72, 0.26];
const RED: [f32; 3] = [1.0, 0.36, 0.3];
const GOLD: [f32; 3] = [1.0, 0.84, 0.3];
const GERM: [f32; 3] = [0.45, 1.0, 0.32];
const CAT: [f32; 3] = [0.62, 0.86, 0.4];
const BACON: [f32; 3] = [0.98, 0.5, 0.44];
/// Kung Fu: each style's colour (`kungfu::Style`'s order), chi's, and the iron body's.
pub const STYLE_RGB: [[f32; 3]; 7] = [[1.0, 0.56, 0.16], [0.86, 0.93, 1.0], [0.42, 0.92, 0.5], [0.32, 0.86, 0.9], [0.98, 0.82, 0.3], [0.96, 0.5, 0.74], [0.68, 0.8, 0.38]];
const CHI: [f32; 3] = [0.55, 0.82, 1.0];
const IRON: [f32; 3] = [0.74, 0.82, 0.9];
/// Seconds a feedback icon stays over the crosshair, and a combo's count pops after a hit.
pub const FEEL_TIME: f32 = 0.7;
const POP_TIME: f32 = 0.2;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Icons>().init_resource::<Hud>().add_systems(Update, update.after(crate::arena::tick));
    }
}

// ----- What is shown: worked out from the round, without the screen.

/// An ability's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Charge {
    Ready,
    /// In use, with the seconds left (None: for as long as it applies).
    Active(Option<f32>),
    /// Cooling down: the seconds until it is ready.
    Cooling(f32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ability {
    pub icon: Icon,
    pub charge: Charge,
    pub tint: [f32; 3],
}
/// What is in hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Gun(u8),
    /// A loose object (Forts' building blocks).
    Carry,
    Bacon,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounds {
    /// The rounds in the magazine, what it holds, and the rounds in reserve (kitchen-sim magazine.rs).
    Count { now: i32, full: i32, reserve: i32 },
    Unlimited,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Arms {
    pub held: Held,
    pub rounds: Option<Rounds>,
    /// The other guns held, small: each with its rounds, magazine and reserve together (None: unlimited).
    pub others: Vec<(u8, Option<i32>)>,
    /// Behind the forcefield: the guns are locked.
    pub locked: bool,
    /// A reload under way: how far, in eighths (a bar that fills: reload.rs).
    pub reload: Option<u8>,
    /// Rounds just put into a reserve by a pickup walked over: the gun and how many (a pop: an ammunition icon and +N).
    pub gained: Option<(u8, i32)>,
}
/// An icon and a number (or nothing): a chip of the top line. `key` is a key cap shown before it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub icon: Icon,
    pub tint: [f32; 3],
    pub text: String,
    pub key: Option<&'static str>,
}
impl Chip {
    fn new(icon: Icon, tint: [f32; 3], text: impl Into<String>) -> Self {
        Self { icon, tint, text: text.into(), key: None }
    }
}
/// The objective: an icon in a colour, the way to it (radians from straight ahead, clockwise) and how far it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Objective {
    pub icon: Icon,
    pub tint: [f32; 3],
    pub bearing: f32,
    pub distance: f32,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Top {
    /// Forts: each team's rounds won and the rounds a match needs, and this player's team.
    pub teams: Option<([u32; 2], u32, Option<u8>)>,
    pub chips: Vec<Chip>,
    pub objective: Option<Objective>,
    /// Kung Fu: this player's KOs, the KOs to win, and its colour (pips).
    pub kos: Option<(u32, u32, [f32; 3])>,
}
/// Kung Fu: this player's fight, bottom right where the gun would be.
#[derive(Clone, Debug, PartialEq)]
pub struct Fight {
    /// Its style (`kungfu::Style` as its number).
    pub style: u8,
    /// Chi, 0 to 100, and whether it is full (the special is ready: the meter glows), with the special's key then.
    pub chi: f32,
    pub full: bool,
    pub key: Key,
    /// Its guard, 0 to 100: shown under the health while it is down.
    pub guard: f32,
    /// A Toad standing in its iron body.
    pub iron: bool,
    /// Hits in its combo now (shown from two up).
    pub combo: u32,
}
/// A key (its cap's label: "E", "Q") or a mouse button, as a prompt shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Cap(&'static str),
    Mouse(Icon),
    /// By touch: no key, the button is on the screen.
    None,
}
/// One thing a key would do here: its key, the icon of what it does, and a number or nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub key: Key,
    pub icon: Icon,
    pub tint: [f32; 3],
    pub text: String,
}
/// A moment in the middle of the screen: one to three words, and under them an icon and a number, or a key.
#[derive(Clone, Debug, PartialEq)]
pub struct Moment {
    pub words: String,
    pub tint: [f32; 3],
    pub under: Option<(Key, Option<Icon>, String)>,
}
/// Everything the display shows this frame.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Show {
    pub up: bool,
    pub health: Option<f32>,
    pub abilities: Vec<Ability>,
    pub arms: Option<Arms>,
    /// Kung Fu's in place of the arms.
    pub fight: Option<Fight>,
    pub top: Top,
    pub prompts: Vec<Prompt>,
    pub moment: Option<Moment>,
    /// Dead: whom the death camera watches.
    pub watching: Option<(String, [f32; 3])>,
}

/// What the display is told besides the round.
#[derive(Clone, Default)]
pub struct Context {
    /// The touch controls are showing: prompts have no keys.
    pub touch: bool,
    /// The players between rounds in the fridge.
    pub holding: bool,
    /// The death camera's hotdog.
    pub watching: Option<u32>,
    /// The colours players chose (the lobby's), by id.
    pub colours: HashMap<u32, [f32; 3]>,
    /// The moment due from what just happened (`Hud::moment`).
    pub moment: Option<Moment>,
    /// What putting down would do (preview.rs), for the prompt.
    pub put: Option<crate::hints::Put>,
    /// What each key does (controls.rs), for the prompts' key caps.
    pub bindings: crate::controls::Bindings,
    /// Rounds just put into a reserve by a pickup walked over (reload.rs `pop`): the gun and how many.
    pub pop: Option<(u8, i32)>,
}
impl Context {
    /// The key cap (or the mouse button's icon) of `action`'s main input; nothing by touch, or when it has none.
    pub fn key(&self, action: crate::controls::Action) -> Key {
        use crate::controls::Input;
        match self.bindings.cap(action).filter(|_| !self.touch) {
            Some(Input::Mouse(MouseButton::Left)) => Key::Mouse(Icon::MouseLeft),
            Some(Input::Mouse(MouseButton::Right)) => Key::Mouse(Icon::MouseRight),
            Some(input) => Key::Cap(input.label()),
            None => Key::None,
        }
    }
    /// The same as a key cap's label, for a chip.
    fn cap(&self, action: crate::controls::Action) -> Option<&'static str> {
        self.bindings.cap(action).map(|input| input.label())
    }
}

/// A hotdog's colour: its team's in Forts, otherwise the one its player chose, or its place's.
pub fn colour(p: &Player, forts: bool, chosen: Option<[f32; 3]>) -> [f32; 3] {
    if forts { crate::forts::TEAM_RGB[(p.team as usize).min(1)] } else { chosen.unwrap_or(feast::COLORS[(p.id as usize).saturating_sub(1) % 5]) }
}
/// "m:ss".
pub fn clock(seconds: f32) -> String {
    let s = seconds.max(0.).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}
/// The way from `from` (looking along `yaw`) to `to`, flat: radians clockwise from straight ahead, and metres.
pub fn bearing(from: Vec3, yaw: f32, to: Vec3) -> (f32, f32) {
    let d = Vec2::new(to.x - from.x, to.z - from.z);
    // On the counter seen from above (x, z): the view looks along `ahead`, and `right` is its right.
    let (ahead, right) = (Vec2::new(-yaw.sin(), -yaw.cos()), Vec2::new(yaw.cos(), -yaw.sin()));
    (d.dot(right).atan2(d.dot(ahead)), d.length())
}

impl Show {
    /// What the display shows of `g`'s round for this player.
    pub fn of(g: &Game, cx: &Context) -> Show {
        let table = crate::cards::at_table(g);
        let mut show = Show { up: g.playing() && !table && !cx.holding, ..default() };
        if !show.up {
            return show;
        }
        let forts = g.fort.is_some();
        let me = g.roster.iter().find(|p| p.id == g.me());
        let alive = me.is_some_and(|p| p.alive);
        let first = alive && !g.demo;
        let tint = |p: &Player| colour(p, forts, cx.colours.get(&p.id).copied());
        if first {
            let me = me.unwrap();
            show.health = Some(me.hp.clamp(0., 100.));
            show.abilities = abilities(g, me);
            show.prompts = prompts(g, me, cx);
            if g.kung_fu() {
                // Hands and feet: no gun, no rounds, no stim (the stim's key dodges).
                show.fight = fight(g, me, cx);
                show.abilities.retain(|a| a.icon != Icon::Stim);
            } else {
                show.arms = Some(arms(g, me, cx));
            }
        }
        show.top = top(g, me, cx);
        show.moment = cx.moment.clone().or_else(|| lasting(g, me, cx));
        if !alive && !g.demo {
            show.watching = cx.watching.and_then(|id| g.roster.iter().find(|p| p.id == id)).map(|p| (p.name.clone(), tint(p)));
        }
        show
    }
}

fn abilities(g: &Game, me: &Player) -> Vec<Ability> {
    let mut out = vec![];
    let stim = if me.stim > 0. {
        Charge::Active(Some(me.stim))
    } else if me.stim_wait > 0. {
        Charge::Cooling(me.stim_wait)
    } else {
        Charge::Ready
    };
    out.push(Ability { icon: Icon::Stim, charge: stim, tint: MINT });
    if g.prone || g.crouch {
        out.push(Ability { icon: if g.prone { Icon::Prone } else { Icon::Crouch }, charge: Charge::Active(None), tint: WHITE });
    }
    if me.shield > 0. {
        out.push(Ability { icon: Icon::Shield, charge: Charge::Active(Some(me.shield)), tint: [0.5, 0.8, 1.] });
    }
    if g.infected {
        out.push(Ability { icon: Icon::Infected, charge: Charge::Active(None), tint: GERM });
    }
    // The famine: health drains, and only eating brings it back.
    if g.phase_name() == "FreeForAll" && !g.feast.rules.active() {
        out.push(Ability { icon: Icon::Eat, charge: Charge::Active(None), tint: AMBER });
    }
    out
}

/// Whether a fighter stands in a Toad's iron body.
pub fn iron(f: &kitchen_sim::kungfu::FighterView) -> bool {
    f.iron
}
fn fight(g: &Game, me: &Player, cx: &Context) -> Option<Fight> {
    let f = g.fighter(me.id)?;
    let full = f.chi >= kitchen_sim::kungfu::CHI_MAX - 1e-3;
    Some(Fight {
        style: f.style.min(6),
        chi: f.chi.clamp(0., kitchen_sim::kungfu::CHI_MAX),
        full,
        key: if full { cx.key(crate::controls::Action::Special) } else { Key::None },
        guard: f.guard.clamp(0., kitchen_sim::kungfu::GUARD_MAX),
        iron: iron(f),
        combo: f.combo,
    })
}

fn arms(g: &Game, me: &Player, cx: &Context) -> Arms {
    let unlimited = g.feast.rules.unlimited();
    let count = |w: usize| if unlimited { None } else { Some(g.ammo[w.min(feast::GUNS - 1)].max(0) + g.mag.reserve[w.min(feast::GUNS - 1)]) };
    let held = if g.weapon == Weapon::Laden {
        if me.carry >= 0 { Held::Carry } else { Held::Bacon }
    } else {
        Held::Gun(g.weapon as u8)
    };
    let rounds = match held {
        Held::Gun(_) if unlimited => Some(Rounds::Unlimited),
        Held::Gun(w) => Some(Rounds::Count { now: g.ammo[w as usize], full: feast::FULL[w as usize], reserve: g.mag.reserve[w as usize] }),
        _ => None,
    };
    let others = crate::arena::held_guns(&g.owned).into_iter().filter(|w| *w != g.weapon).map(|w| (w as u8, count(w as usize))).collect();
    let reload = g.mag.reload.filter(|r| r.gun == g.weapon as usize && r.left > 0.).map(|r| (r.progress() * 8.).floor().min(8.) as u8);
    Arms { held, rounds, others, locked: g.locked(), reload, gained: cx.pop }
}

fn top(g: &Game, me: Option<&Player>, cx: &Context) -> Top {
    let mut top = Top::default();
    let rules = &g.feast.rules;
    let phase = g.phase_name();
    if let Some(state) = g.fort.as_ref().filter(|_| !rules.active()) {
        let team = me.map(|p| p.team.min(1));
        top.teams = Some((state.scores, state.rounds.max(1), team));
        let people = g.roster.iter().filter(|p| !p.bot && p.alive).count();
        let ready = g.roster.iter().filter(|p| !p.bot && p.alive && p.ready).count();
        match phase {
            "Build" => {
                top.chips.push(Chip::new(Icon::Hammer, WHITE, clock(state.ends - g.elapsed)));
                let mine = me.is_some_and(|p| p.ready);
                let key = (!mine && !cx.touch && !g.demo && me.is_some_and(|p| p.alive)).then(|| cx.cap(crate::controls::Action::Ready)).flatten();
                top.chips.push(Chip { key, ..Chip::new(Icon::Check, if mine { MINT } else { DIM }, format!("{ready}/{people}")) });
            }
            "Fight" => {
                let eye = g.position;
                let holder = g.roster.iter().find(|p| state.bacon.state == HELD && p.id == state.bacon.holder);
                let rival = |p: &Player| crate::forts::TEAM_RGB[(p.team as usize).min(1)];
                let (icon, tint, to) = match holder {
                    Some(p) if p.id == g.me() => {
                        let home = g.feast.level.fort.as_ref().map(|f| {
                            let r = f.bases[(p.team as usize).min(1)];
                            Vec3::new(((r[0] + r[2]) / 2.) as f32, eye.y, ((r[1] + r[3]) / 2.) as f32)
                        });
                        (Icon::Home, rival(p), home)
                    }
                    Some(p) => (Icon::Bacon, rival(p), Some(Vec3::from_array(p.p))),
                    None => (Icon::Bacon, BACON, Some(Vec3::from_array(state.bacon.p))),
                };
                if let Some(to) = to {
                    let (angle, far) = bearing(eye, g.yaw, to);
                    top.objective = Some(Objective { icon, tint, bearing: angle, distance: far });
                }
                if holder.is_none() && state.bacon.state == DROPPED {
                    top.chips.push(Chip::new(Icon::Clock, BACON, format!("{}", (kitchen_sim::forts::RETURN - state.bacon.idle).ceil().max(0.) as u32)));
                } else if state.stale < 10. {
                    top.chips.push(Chip::new(Icon::Cat, CAT, format!("{}", state.stale.ceil().max(0.) as u32)));
                }
            }
            _ => {}
        }
        return top;
    }
    let kills = me.map_or(0, |p| p.kills);
    // Kung Fu: the clock, this player's KOs as pips of the KOs to win, the leader's KOs.
    if let Some(view) = &g.kungfu {
        if view.ends > 0. {
            top.chips.push(Chip::new(Icon::Clock, WHITE, clock(view.ends - g.elapsed)));
        }
        if let Some(me) = me {
            top.kos = Some((kills, view.goal.max(1), colour(me, false, cx.colours.get(&me.id).copied())));
        }
        let leader = g.roster.iter().filter(|p| p.kills > 0).max_by_key(|p| (p.kills, std::cmp::Reverse(p.id)));
        if let Some(leader) = leader {
            let tint = if Some(leader.id) == me.map(|p| p.id) { GOLD } else { colour(leader, false, cx.colours.get(&leader.id).copied()) };
            top.chips.push(Chip::new(Icon::Crown, tint, format!("{}", leader.kills)));
        }
        return top;
    }
    if rules.active() {
        if rules.ends() {
            top.chips.push(Chip::new(Icon::Clock, WHITE, clock(g.feast.ends - g.elapsed)));
            top.chips.push(Chip::new(Icon::Skull, WHITE, format!("{kills}/{}", kitchen_sim::rules::FFA_KILLS)));
            let leader = g.roster.iter().filter(|p| p.kills > 0).max_by_key(|p| (p.kills, std::cmp::Reverse(p.id)));
            if let Some(leader) = leader {
                let tint = if Some(leader.id) == me.map(|p| p.id) { GOLD } else { colour(leader, false, cx.colours.get(&leader.id).copied()) };
                top.chips.push(Chip::new(Icon::Crown, tint, format!("{}", leader.kills)));
            }
        } else {
            top.chips.push(Chip::new(Icon::Skull, WHITE, format!("{kills}")));
        }
        return top;
    }
    // The siege: the wave's clock, then the cats left; the hotdogs alive; this player's kills.
    let alive = g.roster.iter().filter(|p| p.alive).count();
    match phase {
        "Warmup" => top.chips.push(Chip::new(Icon::Clock, WHITE, clock(g.feast.warmup - g.elapsed))),
        "Siege" => top.chips.push(Chip::new(Icon::Cat, CAT, format!("{}", g.cats_left()))),
        _ => {}
    }
    top.chips.push(Chip::new(Icon::Hotdog, WHITE, format!("{alive}")));
    top.chips.push(Chip::new(Icon::Skull, DIM, format!("{}", g.kills)));
    top
}

fn prompts(g: &Game, me: &Player, cx: &Context) -> Vec<Prompt> {
    use crate::hints::{Grasp, Pad, Put, Reach};
    use crate::controls::Action;
    let p = |key: Key, icon: Icon, text: String| Prompt { key, icon, tint: WHITE, text };
    let reach = Reach { put: cx.put, ..Reach::of(g, cx.touch) };
    let mut out = vec![];
    if reach.carrying.is_some() {
        let (icon, text) = match reach.put {
            Some(Put::Drape(n)) => (Icon::Drape, if n > 0 { format!("{n}") } else { String::new() }),
            Some(Put::Still { locks: true, .. }) => (Icon::Lock, String::new()),
            _ => (Icon::PutDown, String::new()),
        };
        out.push(p(cx.key(Action::Carry), icon, text));
        out.push(p(cx.key(Action::Throw), Icon::Throw, String::new()));
    }
    match reach.grasp {
        Some(Grasp::Loose(_)) => out.push(p(cx.key(Action::Carry), Icon::Hand, String::new())),
        Some(Grasp::Drape) => out.push(p(cx.key(Action::Carry), Icon::Undrape, String::new())),
        None => {}
    }
    match &reach.pad {
        Some(Pad::Gun(_)) => {
            let at = g.position + Vec3::Y * 0.04;
            let w = g.spawns().into_iter().filter(|s| s.ready <= g.elapsed).find(|s| Vec3::from_array(s.p).distance(at) < 0.15).map_or(1, |s| s.weapon.max(1));
            out.push(p(cx.key(Action::Use), Icon::Gun(w.min(7) as u8), String::new()));
        }
        // (A box of ammunition is taken by walking over it: no key.)
        _ => {}
    }
    // An electric griddle's dial in reach, and faced: use switches it (the icon is bright while it is on).
    if let Some(i) = g.feast.griddle_near(g.position.to_array(), g.yaw) {
        let tint = if g.feast.griddles[i].on { AMBER } else { WHITE };
        out.push(Prompt { key: cx.key(Action::Use), icon: Icon::Power, tint, text: String::new() });
    }
    // An empty pad: the seconds until its pickup is back.
    if reach.pad.is_none() && !g.infected {
        let at = g.position + Vec3::Y * 0.04;
        if let Some(s) = g.spawns().into_iter().find(|s| s.ready > g.elapsed && Vec3::from_array(s.p).distance(at) < 0.15) {
            let icon = if s.weapon == kitchen_sim::map::AMMO_BOX { Icon::AmmoBox } else { Icon::Gun(s.weapon.clamp(1, 7) as u8) };
            let left = s.ready - g.elapsed;
            let text = if crate::spawns::progress(s.ready, s.respawn, g.elapsed).is_some() { format!("{}", left.ceil() as u32) } else { String::new() };
            out.push(Prompt { key: Key::None, icon, tint: DIM, text });
        }
    }
    // The famine: E eats a fallen hotdog in reach, F bites a living one.
    if g.phase_name() == "FreeForAll" && !g.feast.rules.active() {
        let at = Vec3::from_array(me.p);
        let near = |dead: bool| g.roster.iter().any(|t| t.id != me.id && !t.consumed && t.alive != dead && Vec3::from_array(t.p).distance(at) < 0.17);
        if near(true) {
            out.push(p(cx.key(Action::Use), Icon::Eat, String::new()));
        }
        if near(false) {
            out.push(p(cx.key(Action::Melee), Icon::Bite, String::new()));
        }
    }
    out
}

/// A moment that lasts as long as the round's state does: the round over, a Forts round between rounds, dead and
/// waiting to respawn.
fn lasting(g: &Game, me: Option<&Player>, cx: &Context) -> Option<Moment> {
    let phase = g.phase_name();
    let words = |w: &str, tint: [f32; 3]| Some(Moment { words: w.into(), tint, under: None });
    if let Some(state) = g.fort.as_ref().filter(|_| matches!(phase, "Rest" | "RoundOver")) {
        let team = me.map(|p| p.team.min(1) as i32);
        return match (state.champion, state.winner) {
            (Some(t), _) if team == Some(t as i32) => words("VICTORY", GOLD),
            (Some(_), _) if team.is_some() && !g.demo => words("DEFEAT", RED),
            (Some(t), _) => words(if t == 0 { "MUSTARD WINS" } else { "KETCHUP WINS" }, crate::forts::TEAM_RGB[(t as usize).min(1)]),
            (None, Some(t)) if t < 0 => words("DRAW", WHITE),
            (None, Some(t)) if team == Some(t) && !g.demo => words("ROUND WON", GOLD),
            (None, Some(_)) if !g.demo => words("ROUND LOST", RED),
            (None, Some(t)) => words(if t == 0 { "MUSTARD ROUND" } else { "KETCHUP ROUND" }, crate::forts::TEAM_RGB[(t as usize).min(1)]),
            _ => None,
        };
    }
    if phase == "RoundOver" {
        // (Its key cap shows by touch too, as it always has: the page's New round button is the same.)
        let enter = Context { touch: false, ..cx.clone() }.key(crate::controls::Action::NewRound);
        let again = (!g.network && !g.demo && !g.feast.rules.ffa).then(|| (enter, None, String::new()));
        let mut m = match g.feast.winner.and_then(|id| g.roster.iter().find(|p| p.id == id)) {
            Some(p) if Some(p.id) == me.map(|m| m.id) && !g.demo => Moment { words: "YOU WIN".into(), tint: GOLD, under: None },
            Some(p) => Moment { words: format!("{} WINS", p.name.to_uppercase()), tint: WHITE, under: None },
            None if g.feast.rules.ffa => Moment { words: "DRAW".into(), tint: WHITE, under: None },
            None => Moment { words: "NO SURVIVORS".into(), tint: RED, under: None },
        };
        m.under = again;
        return Some(m);
    }
    // Dead in Free For All (or the sandbox): back in a few seconds.
    match me {
        Some(p) if !p.alive && !g.demo && p.respawn > 0. => {
            Some(Moment { words: "YOU DIED".into(), tint: RED, under: Some((Key::None, Some(Icon::Clock), format!("{:.1}", p.respawn))) })
        }
        _ => None,
    }
}

/// What killed `victim`, for the feed: the killer's icon and colour (None when nobody did: hunger, a fall), and the
/// icon of how.
pub fn kill(victim: &Player, roster: &[Player], forts: bool, colours: &HashMap<u32, [f32; 3]>) -> Kill {
    let record = victim.last_damage;
    let killer = roster.iter().find(|p| p.id == record.attacker && record.attacker != 0);
    let them = killer.map(|p| (Icon::Hotdog, colour(p, forts, colours.get(&p.id).copied())));
    // A round in the face has its own mark (the gun's kind under it: kitchen_sim feast.rs FACE_SHOT).
    if feast::faced(record.kind) {
        return Kill { at: 0., killer: them, killer_id: killer.map(|p| p.id), how: Some(Icon::FaceShot), victim: victim.name.clone(), victim_id: victim.id, tint: colour(victim, forts, colours.get(&victim.id).copied()) };
    }
    let (who, how) = match record.kind {
        1..=5 => (them, Some(Icon::Gun(record.kind))),
        feast::SYRUP => (them, Some(Icon::Gun(6))),
        feast::MAYONNAISE => (them, Some(Icon::Gun(7))),
        feast::MELEE => (them, Some(Icon::Strike)),
        feast::BACKSTAB => (them, Some(Icon::Backstab)),
        feast::BURN => (them, Some(Icon::Flame)),
        feast::TURRET => (Some((Icon::Turret, DIM)), None),
        feast::ZOMBIE => (Some((Icon::Cat, CAT)), Some(Icon::Bite)),
        feast::PESTS => (Some((Icon::Flies, CAT)), None),
        feast::HUNGER => (None, Some(Icon::Eat)),
        // (Kung Fu credits a ring-out to the last to hit the faller.)
        feast::FALL => (them, Some(Icon::Fall)),
        kitchen_sim::kungfu::BLOW => (them, Some(Icon::Strike)),
        _ => (them, None),
    };
    Kill { at: 0., killer: who, killer_id: killer.map(|p| p.id), how, victim: victim.name.clone(), victim_id: victim.id, tint: colour(victim, forts, colours.get(&victim.id).copied()) }
}
/// A line of the kill feed.
#[derive(Clone, Debug, PartialEq)]
pub struct Kill {
    pub at: f32,
    pub killer: Option<(Icon, [f32; 3])>,
    pub killer_id: Option<u32>,
    pub how: Option<Icon>,
    pub victim: String,
    pub victim_id: u32,
    pub tint: [f32; 3],
}

// ----- What happened: the display's memory from frame to frame.

/// The display's state: what it built, for which size, and what it remembers of the round (who was alive, the kills,
/// the moment due).
#[derive(Resource, Default)]
pub struct Hud {
    /// The root and the scale (logical and the window's scale factor) it was built for.
    built: Option<(Entity, f32, f32)>,
    regions: HashMap<Region, Entity>,
    /// What each region shows now, so that a region is rebuilt only when that changes.
    shown: HashMap<Region, String>,
    /// The arrow of the objective, turned every frame.
    arrow: Option<Entity>,
    alive: HashMap<u32, bool>,
    phase: &'static str,
    round: (u32, u32),
    infected: bool,
    bacon: bool,
    pub feed: Vec<Kill>,
    /// A moment, and the time (`Time<Real>`) until which it shows.
    moment: Option<(Moment, f32)>,
    /// The rounds of the gun in hand last frame, and until when its count flashes (a pickup), and the lock (a shot
    /// refused behind the forcefield).
    rounds: Option<(Weapon, i32)>,
    flash: f32,
    refused: f32,
    /// Kung Fu: the last event taken, this player's style and combo last frame, the feedback over the crosshair (its
    /// icon, colour and when), when the combo's count last went up, and the entities that change every frame (the
    /// chi meter's fill, while it glows; the combo's count, while it pops; the feedback's icon, while it fades).
    told: u32,
    style: Option<u8>,
    combo: u32,
    pub feel: Option<(Icon, [f32; 3], f32)>,
    pub popped: f32,
    glow: Option<Entity>,
    pop: Option<Entity>,
    fade: Option<Entity>,
}
impl Hud {
    /// Shows `words` in the middle for `seconds` from `now`.
    pub fn say(&mut self, words: &str, tint: [f32; 3], now: f32, seconds: f32) {
        self.moment = Some((Moment { words: words.into(), tint, under: None }, now + seconds));
    }
    /// Takes in what changed since the last frame: deaths for the feed, this player's own death, a new phase, an
    /// infection, the bacon, a pickup.
    pub fn hear(&mut self, g: &Game, now: f32, colours: &HashMap<u32, [f32; 3]>) {
        let forts = g.fort.is_some();
        let phase = g.phase_name();
        let round = (g.feast.round, g.fort.as_ref().map_or(0, |f| f.match_no));
        let fresh = round != self.round || (phase != self.phase && matches!(phase, "Warmup" | "Build"));
        if fresh {
            self.alive.clear();
            self.round = round;
        }
        for p in &g.roster {
            let was = self.alive.insert(p.id, p.alive);
            if was == Some(true) && !p.alive {
                let mut line = kill(p, &g.roster, forts, colours);
                // Kung Fu credits a KO to whoever did the victim the most harm (interface.md): the feed shows that one.
                let credit = g.kungfu.as_ref().and_then(|v| v.events.iter().rev().find(|e| e.kind == Some(kitchen_sim::kungfu::EventKind::Ko) && e.to == p.id)).map(|e| e.by);
                if let Some(by) = credit.filter(|by| *by != 0 && Some(*by) != line.killer_id) {
                    if let Some(them) = g.roster.iter().find(|q| q.id == by) {
                        (line.killer, line.killer_id) = (Some((Icon::Hotdog, colour(them, forts, colours.get(&them.id).copied()))), Some(by));
                    }
                }
                self.feed.push(Kill { at: now, ..line });
                if p.id == g.me() && !g.demo && p.respawn <= 0. {
                    self.say("YOU DIED", RED, now, 3.);
                }
            }
        }
        self.feed.retain(|k| now - k.at < FEED_TIME);
        while self.feed.len() > FEED_MAX {
            self.feed.remove(0);
        }
        if phase != self.phase && !self.phase.is_empty() || (fresh && self.phase.is_empty()) {
            let rules = &g.feast.rules;
            match phase {
                "Build" => self.say("BUILD", WHITE, now, 2.),
                "Fight" => self.say("STEAL THE BACON", BACON, now, 2.5),
                "Siege" => self.say("WAVE", CAT, now, 2.),
                "FreeForAll" if rules.ffa => self.say("FREE FOR ALL", WHITE, now, 2.5),
                "FreeForAll" if !rules.active() => self.say("FREE FOR ALL", AMBER, now, 3.),
                _ => {}
            }
        }
        self.phase = phase;
        if g.infected && !self.infected {
            self.say("INFECTED", GERM, now, 2.5);
        }
        self.infected = g.infected;
        let bacon = g.fort.as_ref().is_some_and(|f| f.bacon.state == HELD && f.bacon.holder == g.me());
        if bacon && !self.bacon && !g.demo {
            self.say("RUN HOME", BACON, now, 2.);
        }
        self.bacon = bacon;
        // A pickup: the rounds of the gun in hand went up, magazine and reserve together (a reload moves them from one
        // to the other).
        let w = (g.weapon as usize).min(feast::GUNS - 1);
        let now_rounds = (g.weapon, g.ammo[w] + g.mag.reserve[w]);
        if let Some((w, n)) = self.rounds {
            if w == now_rounds.0 && now_rounds.1 > n {
                self.flash = now + 0.6;
            }
        }
        self.rounds = Some(now_rounds);
        self.fight(g, now);
    }
    /// Kung Fu: what happened to this player since the last frame (the events not yet taken: a blow blocked, a parry, a
    /// guard broken, by it or to it), a new style (its name for a moment) and a longer combo (its count pops).
    fn fight(&mut self, g: &Game, now: f32) {
        use kitchen_sim::kungfu::EventKind;
        let Some(view) = &g.kungfu else {
            (self.told, self.style, self.combo, self.feel) = (0, None, 0, None);
            return;
        };
        let me = g.me();
        // (The ids start again from 1 each round.)
        if view.events.iter().map(|e| e.id).max().unwrap_or(0) < self.told {
            self.told = 0;
        }
        for e in view.events.iter().filter(|e| e.id > self.told) {
            let (mine, theirs) = (e.by == me, e.to == me);
            let feel = match e.kind {
                Some(EventKind::Blocked) if theirs => Some((Icon::Guard, WHITE)),
                Some(EventKind::Blocked) if mine => Some((Icon::Guard, DIM)),
                Some(EventKind::Parried) if mine => Some((Icon::Parry, GOLD)),
                Some(EventKind::Parried) if theirs => Some((Icon::Parry, RED)),
                Some(EventKind::Guardbroken) if mine => Some((Icon::Broken, GOLD)),
                Some(EventKind::Guardbroken) if theirs => Some((Icon::Broken, RED)),
                // A Toad's iron body took a strike: it rings round the Toad's animal, and the striker bounced off.
                Some(EventKind::Absorbed) if theirs => Some((Icon::Halo, IRON)),
                Some(EventKind::Absorbed) if mine => Some((Icon::Halo, RED)),
                _ => None,
            };
            if let Some((icon, tint)) = feel {
                self.feel = Some((icon, tint, now));
            }
        }
        self.told = self.told.max(view.events.iter().map(|e| e.id).max().unwrap_or(0));
        let Some(f) = view.fighter(me) else { return };
        if self.style.is_some_and(|s| s != f.style) && !g.demo {
            let style = f.style.min(6);
            self.say(&kitchen_sim::kungfu::Style::NAMES.get(style as usize).copied().unwrap_or("").to_uppercase(), STYLE_RGB[style as usize], now, 0.9);
        }
        self.style = Some(f.style);
        if f.combo > self.combo && f.combo >= 2 {
            self.popped = now;
        }
        self.combo = f.combo;
    }
    /// The moment due now, if any.
    pub fn moment(&self, now: f32) -> Option<Moment> {
        self.moment.as_ref().filter(|(_, until)| now < *until).map(|(m, _)| m.clone())
    }
}

// ----- On the screen.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Region {
    Vitals,
    Arms,
    Top,
    Objective,
    Feed,
    Prompt,
    Moment,
    Watch,
    /// Kung Fu: the combo's count, right of the crosshair, and the feedback over it.
    Combo,
    Feel,
}
/// The display's root: everything else hangs from it.
#[derive(Component)]
pub struct Root;
/// Each region's node.
#[derive(Component)]
struct Area;

/// The sizes: `s` is the logical scale (the window's height over SCALE) and `sf` the window's scale factor.
#[derive(Clone, Copy)]
struct Size {
    s: f32,
    sf: f32,
}
impl Size {
    fn px(&self, base: f32) -> Val {
        Val::Px((base * self.s * self.sf).round() / self.sf)
    }
    fn font(&self, base: f32) -> TextFont {
        TextFont { font_size: FontSize::Px((base * self.s).round()), ..default() }
    }
}
fn rgb(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}
fn rgba(c: [f32; 3], a: f32) -> Color {
    Color::srgba(c[0], c[1], c[2], a)
}

/// Every frame after the round: the display built (again, when the window's size changes) and filled in.
pub fn update(world: &mut World) {
    let now = world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs());
    let (s, sf) = world.query::<&Window>().iter(world).next().map_or((1., 1.), |w| ((w.height() / SCALE).clamp(0.8, 4.), w.scale_factor()));
    let Some(g) = world.remove_resource::<Game>() else {
        return;
    };
    let colours: HashMap<u32, [f32; 3]> = world.get_resource::<crate::lobby::Lobby>().map_or_else(HashMap::new, |lobby| g.roster.iter().filter_map(|p| lobby.colour(p.id).map(|c| (p.id, c))).collect());
    let mut hud = world.remove_resource::<Hud>().unwrap_or_default();
    if g.playing() {
        hud.hear(&g, now, &colours);
    }
    // A shot refused behind the forcefield: the lock flashes.
    let bindings = world.get_resource::<crate::controls::Bindings>().cloned().unwrap_or_default();
    let fired = match (world.get_resource::<ButtonInput<KeyCode>>(), world.get_resource::<ButtonInput<MouseButton>>()) {
        (Some(keys), Some(mouse)) => bindings.just(crate::controls::Action::Fire, keys, mouse),
        _ => false,
    };
    if g.locked() && fired {
        hud.refused = now + 0.5;
    }
    let cx = Context {
        touch: crate::touch::on(),
        holding: world.get_resource::<crate::lobby::Lobby>().is_some_and(|l| l.holding),
        watching: world.get_resource::<crate::deathcam::DeathCam>().and_then(crate::deathcam::DeathCam::watching),
        colours,
        moment: hud.moment(now),
        put: world.get_resource::<crate::preview::Preview>().and_then(|p| p.spot(g.frame())).map(|s| s.put()),
        bindings,
        pop: crate::reload::pop(&g, now).map(|(w, n, _)| (w, n)),
    };
    let show = Show::of(&g, &cx);
    let size = Size { s, sf };
    let rebuild = hud.built.is_none_or(|(root, bs, bsf)| (bs - s).abs() > 1e-3 || (bsf - sf).abs() > 1e-3 || world.get_entity(root).is_err());
    if rebuild {
        if let Some((root, ..)) = hud.built.take() {
            if let Ok(e) = world.get_entity_mut(root) {
                e.despawn();
            }
        }
        let root = frame(world, &mut hud, size);
        hud.built = Some((root, s, sf));
        hud.shown.clear();
    }
    let (root, ..) = hud.built.unwrap();
    world.entity_mut(root).insert(if show.up { Visibility::Inherited } else { Visibility::Hidden });
    if show.up {
        fill(world, &mut hud, &show, size, now);
    }
    world.insert_resource(hud);
    world.insert_resource(g);
}

/// The root and an empty node for each region, where it goes.
fn frame(world: &mut World, hud: &mut Hud, z: Size) -> Entity {
    let edge = 22.;
    let root = world
        .spawn((
            Root,
            Name::new("Head-up display"),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() },
            Visibility::Hidden,
            // (Over the scope's mask, arena.rs ZIndex -1, and the name tags under it.)
            ZIndex(1),
        ))
        .id();
    let place = |region: Region| -> Node {
        let mut node = Node { position_type: PositionType::Absolute, align_items: AlignItems::Center, ..default() };
        match region {
            Region::Vitals => {
                (node.left, node.bottom, node.flex_direction, node.align_items, node.row_gap) = (z.px(edge), z.px(edge - 4.), FlexDirection::Column, AlignItems::FlexStart, z.px(8.));
            }
            Region::Arms => {
                (node.right, node.bottom, node.flex_direction, node.align_items, node.row_gap) = (z.px(edge), z.px(ARMS_FOOT), FlexDirection::Column, AlignItems::FlexEnd, z.px(6.));
            }
            Region::Top => {
                (node.left, node.right, node.top, node.justify_content, node.column_gap) = (Val::Px(0.), Val::Px(0.), z.px(14.), JustifyContent::Center, z.px(14.));
            }
            Region::Objective => {
                (node.left, node.right, node.top, node.justify_content, node.column_gap) = (Val::Px(0.), Val::Px(0.), z.px(57.), JustifyContent::Center, z.px(6.));
            }
            Region::Feed => {
                (node.right, node.top, node.flex_direction, node.align_items, node.row_gap) = (z.px(edge - 6.), z.px(14.), FlexDirection::Column, AlignItems::FlexEnd, z.px(4.));
            }
            Region::Prompt => {
                (node.left, node.right, node.top, node.margin, node.justify_content, node.column_gap) =
                    (Val::Px(0.), Val::Px(0.), Val::Percent(50.), UiRect::top(z.px(20.)), JustifyContent::Center, z.px(14.));
            }
            Region::Moment => {
                (node.left, node.right, node.top, node.flex_direction, node.row_gap) = (Val::Px(0.), Val::Px(0.), Val::Percent(30.), FlexDirection::Column, z.px(6.));
            }
            Region::Watch => {
                (node.left, node.right, node.bottom, node.justify_content, node.column_gap) = (Val::Px(0.), Val::Px(0.), z.px(edge + 8.), JustifyContent::Center, z.px(8.));
            }
            Region::Combo => {
                (node.left, node.top, node.margin, node.column_gap) = (Val::Percent(50.), Val::Percent(50.), UiRect { left: z.px(64.), top: z.px(-46.), ..default() }, z.px(4.));
            }
            Region::Feel => {
                (node.left, node.right, node.top, node.margin, node.justify_content) = (Val::Px(0.), Val::Px(0.), Val::Percent(50.), UiRect::top(z.px(-62.)), JustifyContent::Center);
            }
        }
        node
    };
    hud.regions.clear();
    for region in [Region::Vitals, Region::Arms, Region::Top, Region::Objective, Region::Feed, Region::Prompt, Region::Moment, Region::Watch, Region::Combo, Region::Feel] {
        let e = world.spawn((Area, place(region), Visibility::Inherited)).id();
        world.entity_mut(root).add_child(e);
        hud.regions.insert(region, e);
    }
    root
}

/// Fills each region in, rebuilding the ones whose content changed.
fn fill(world: &mut World, hud: &mut Hud, show: &Show, z: Size, now: f32) {
    let flash = now < hud.flash;
    let refused = now < hud.refused;
    // (Kung Fu's numbers as they show: whole points of chi and guard.)
    let fight = show.fight.as_ref().map(|f| (f.style, f.chi as i32, f.full, f.key, (f.guard as i32).min(100), f.iron));
    let combo = show.fight.as_ref().map_or(0, |f| f.combo);
    let feel = hud.feel.filter(|(.., at)| now - at < FEEL_TIME);
    let parts: [(Region, String); 10] = [
        (Region::Vitals, format!("{:?}{:?}{:?}", show.health.map(|h| h.ceil() as i32), show.abilities.iter().map(|a| (a.icon, charge_label(a.charge))).collect::<Vec<_>>(), fight.map(|f| f.4))),
        (Region::Arms, format!("{:?}{flash}{refused}{fight:?}", show.arms)),
        (Region::Top, format!("{:?}{:?}{:?}", show.top.teams, show.top.chips, show.top.kos)),
        (Region::Objective, format!("{:?}", show.top.objective.map(|o| (o.icon, (o.distance * 10.).round() as i32)))),
        (Region::Feed, format!("{:?}", hud.feed.iter().map(|k| (k.at.to_bits(), &k.victim)).collect::<Vec<_>>())),
        (Region::Prompt, format!("{:?}", show.prompts)),
        (Region::Moment, format!("{:?}", show.moment)),
        (Region::Watch, format!("{:?}", show.watching)),
        (Region::Combo, format!("{combo}")),
        (Region::Feel, format!("{:?}", feel.map(|(icon, tint, at)| (icon, tint, at.to_bits())))),
    ];
    for (region, key) in parts {
        if hud.shown.get(&region) == Some(&key) {
            continue;
        }
        let Some(&area) = hud.regions.get(&region) else { continue };
        if let Ok(mut e) = world.get_entity_mut(area) {
            e.despawn_related::<Children>();
        }
        if region == Region::Objective {
            hud.arrow = None;
        }
        match region {
            Region::Arms => hud.glow = None,
            Region::Combo => hud.pop = None,
            Region::Feel => hud.fade = None,
            _ => {}
        }
        let mut ui = Ui { world, z, kids: vec![] };
        match region {
            Region::Vitals => vitals(&mut ui, show),
            Region::Arms => {
                arms_ui(&mut ui, show, flash, refused);
                hud.glow = fight_ui(&mut ui, show);
            }
            Region::Combo => hud.pop = combo_ui(&mut ui, show),
            Region::Feel => hud.fade = feel.map(|(icon, tint, _)| feel_ui(&mut ui, icon, tint)),
            Region::Top => top_ui(&mut ui, show),
            Region::Objective => {
                if let Some(o) = show.top.objective {
                    hud.arrow = Some(objective_ui(&mut ui, o));
                }
            }
            Region::Feed => feed_ui(&mut ui, &hud.feed),
            Region::Prompt => prompt_ui(&mut ui, &show.prompts),
            Region::Moment => moment_ui(&mut ui, show.moment.as_ref()),
            Region::Watch => watch_ui(&mut ui, show.watching.as_ref()),
        }
        let kids = std::mem::take(&mut ui.kids);
        world.entity_mut(area).add_children(&kids);
        hud.shown.insert(region, key);
    }
    // The objective's arrow turns every frame.
    if let (Some(arrow), Some(o)) = (hud.arrow, show.top.objective) {
        if let Some(mut t) = world.get_mut::<UiTransform>(arrow) {
            t.rotation = Rot2::radians(o.bearing);
        }
    }
    // Kung Fu: full chi pulses, a longer combo pops, the feedback fades.
    if let Some(glow) = hud.glow {
        if let Some(mut c) = world.get_mut::<BackgroundColor>(glow) {
            c.0 = rgb(lighten(GOLD, glow_level(now) * 0.6));
        }
    }
    if let Some(pop) = hud.pop {
        if let Some(mut t) = world.get_mut::<UiTransform>(pop) {
            t.scale = Vec2::splat(pop_scale(now - hud.popped));
        }
    }
    if let (Some(fade), Some((_, tint, at))) = (hud.fade, feel) {
        if let Some(mut image) = world.get_mut::<ImageNode>(fade) {
            image.color = rgba(tint, feel_alpha(now - at));
        }
    }
}
/// How bright full chi glows at `now`: a slow pulse, 0 to 1.
pub fn glow_level(now: f32) -> f32 {
    0.5 + 0.5 * (now * std::f32::consts::TAU * 1.4).sin()
}
/// How big the combo's count is `since` seconds after it went up: half again as big at once, back to its size in POP_TIME.
pub fn pop_scale(since: f32) -> f32 {
    if (0. ..POP_TIME).contains(&since) { 1. + 0.5 * (1. - since / POP_TIME).powi(2) } else { 1. }
}
/// How opaque the feedback is `since` seconds after it: whole, then fading over the last half of FEEL_TIME.
pub fn feel_alpha(since: f32) -> f32 {
    (2. * (1. - since / FEEL_TIME)).clamp(0., 1.)
}
fn charge_label(c: Charge) -> String {
    match c {
        Charge::Ready => "r".into(),
        Charge::Active(Some(s)) | Charge::Cooling(s) => format!("{}", s.ceil() as i32),
        Charge::Active(None) => "a".into(),
    }
}

/// A builder of nodes at the display's size; what it makes at the top level is collected in `kids`.
struct Ui<'w> {
    world: &'w mut World,
    z: Size,
    kids: Vec<Entity>,
}
impl Ui<'_> {
    /// An icon `base` pixels high (at SCALE), drawn for the pixels it takes, tinted.
    fn icon(&mut self, icon: Icon, base: f32, tint: Color) -> Entity {
        let height = (base * self.z.s * self.z.sf).round().max(8.) as u32;
        let handle = self.world.resource_scope(|world, mut icons: Mut<Icons>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            icons.get(&mut images, icon, height)
        });
        let (w, h) = crate::icons::size(icon, height);
        self.world
            .spawn((ImageNode { color: tint, ..ImageNode::new(handle) }, Node { width: Val::Px(w as f32 / self.z.sf), height: Val::Px(h as f32 / self.z.sf), flex_shrink: 0., ..default() }))
            .id()
    }
    fn text(&mut self, text: &str, base: f32, tint: Color) -> Entity {
        let shadow = TextShadow { offset: Vec2::splat((1.2 * self.z.s).max(1.)), color: Color::srgba(0., 0., 0., 0.7) };
        self.world.spawn((Text::new(text), self.z.font(base), TextColor(tint), TextLayout::no_wrap(), shadow)).id()
    }
    /// A key cap: its label in a small dark box with a light edge.
    fn cap(&mut self, label: &str) -> Entity {
        let z = self.z;
        let text = self.world.spawn((Text::new(label), z.font(13.), TextColor(rgb(WHITE)), TextLayout::no_wrap())).id();
        self.world
            .spawn((
                Node {
                    min_width: z.px(22.),
                    height: z.px(22.),
                    padding: UiRect::axes(z.px(5.), Val::Px(0.)),
                    border: UiRect::all(z.px(1.5)),
                    border_radius: BorderRadius::all(z.px(4.)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.72)),
                BorderColor::all(Color::srgba(0.9, 0.93, 0.9, 0.85)),
            ))
            .add_child(text)
            .id()
    }
    fn key(&mut self, key: Key) -> Option<Entity> {
        match key {
            Key::Cap(label) => Some(self.cap(label)),
            Key::Mouse(icon) => Some(self.icon(icon, 22., rgb(WHITE))),
            Key::None => None,
        }
    }
    fn row(&mut self, gap: f32, children: &[Entity]) -> Entity {
        let z = self.z;
        self.world.spawn(Node { align_items: AlignItems::Center, column_gap: z.px(gap), ..default() }).add_children(children).id()
    }
    fn top_level(&mut self, e: Entity) {
        self.kids.push(e);
    }
}

fn vitals(ui: &mut Ui, show: &Show) {
    let z = ui.z;
    if !show.abilities.is_empty() {
        let mut slots = vec![];
        for a in &show.abilities {
            let (alpha, label) = match a.charge {
                Charge::Ready => (1., None),
                Charge::Active(left) => (1., left.map(|s| format!("{}", s.ceil() as i32))),
                Charge::Cooling(s) => (0.38, Some(format!("{}", s.ceil() as i32))),
            };
            let icon = ui.icon(a.icon, 26., rgba(a.tint, alpha));
            let mut kids = vec![icon];
            if let Some(label) = label {
                let colour = if matches!(a.charge, Charge::Cooling(_)) { rgb(DIM) } else { rgb(a.tint) };
                kids.push(ui.text(&label, 15., colour));
            }
            slots.push(ui.row(3., &kids));
        }
        let row = ui.row(12., &slots);
        ui.top_level(row);
    }
    if let Some(hp) = show.health {
        let heart = ui.icon(Icon::Heart, 24., rgb(HEART));
        let colour = crate::tags::health_rgb(hp);
        let fill = ui.world.spawn((Node { width: Val::Percent(hp), height: Val::Percent(100.), border_radius: BorderRadius::all(z.px(3.)), ..default() }, BackgroundColor(rgb(colour)))).id();
        let bar = ui
            .world
            .spawn((
                Node { width: z.px(150.), height: z.px(10.), border: UiRect::all(z.px(1.)), border_radius: BorderRadius::all(z.px(4.)), ..default() },
                BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.7)),
                BorderColor::all(Color::srgba(0.02, 0.02, 0.03, 0.9)),
            ))
            .add_child(fill)
            .id();
        let number = ui.text(&format!("{}", hp.ceil().max(1.) as i32), 24., rgb(lighten(colour, 0.35)));
        let row = ui.row(8., &[heart, bar, number]);
        ui.top_level(row);
    }
    // Kung Fu: the guard, a thin bar under the health's, while it is down (blocks wear it, it comes back by itself).
    if let Some(f) = show.fight.as_ref().filter(|f| f.guard < kitchen_sim::kungfu::GUARD_MAX - 0.5) {
        let shield = ui.icon(Icon::Guard, 16., rgb(DIM));
        let tint = if f.guard < 25. { RED } else { IRON };
        let fill = ui.world.spawn((Node { width: Val::Percent(f.guard), height: Val::Percent(100.), border_radius: BorderRadius::all(z.px(2.)), ..default() }, BackgroundColor(rgb(tint)))).id();
        let bar = ui
            .world
            .spawn((Node { width: z.px(150.), height: z.px(5.), border_radius: BorderRadius::all(z.px(3.)), margin: UiRect::left(z.px(4.)), ..default() }, BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.6))))
            .add_child(fill)
            .id();
        let row = ui.row(8., &[shield, bar]);
        ui.top_level(row);
    }
}

/// Kung Fu, bottom right: the chi meter (its icon, the bar, the special's key once it is full) over the style's animal,
/// ringed while a Toad's iron body is up. Returns the meter's fill while it glows.
fn fight_ui(ui: &mut Ui, show: &Show) -> Option<Entity> {
    let f = show.fight.as_ref()?;
    let z = ui.z;
    let tint = if f.full { GOLD } else { CHI };
    let chi = ui.icon(Icon::Chi, 20., rgb(tint));
    let fill = ui.world.spawn((Node { width: Val::Percent(f.chi), height: Val::Percent(100.), border_radius: BorderRadius::all(z.px(3.)), ..default() }, BackgroundColor(rgb(tint)))).id();
    let bar = ui
        .world
        .spawn((
            Node { width: z.px(118.), height: z.px(10.), border: UiRect::all(z.px(1.)), border_radius: BorderRadius::all(z.px(4.)), ..default() },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.7)),
            BorderColor::all(if f.full { rgba(GOLD, 0.9) } else { Color::srgba(0.02, 0.02, 0.03, 0.9) }),
        ))
        .add_child(fill)
        .id();
    let mut meter = vec![chi, bar];
    if let Some(key) = ui.key(f.key) {
        meter.push(key);
    }
    let meter = ui.row(6., &meter);
    ui.top_level(meter);
    // The animal, a halo round it while the iron body holds (and gold while the special is ready).
    let colour = STYLE_RGB[(f.style as usize).min(6)];
    let animal = ui.icon(Icon::Style(f.style), 44., rgb(colour));
    let holder = ui.world.spawn(Node { width: z.px(58.), height: z.px(58.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).add_child(animal).id();
    if f.iron || f.full {
        let halo = ui.icon(Icon::Halo, 58., rgb(if f.iron { IRON } else { GOLD }));
        ui.world.entity_mut(halo).entry::<Node>().and_modify(|mut n| {
            (n.position_type, n.left, n.top) = (PositionType::Absolute, Val::Px(0.), Val::Px(0.));
        });
        ui.world.entity_mut(holder).add_child(halo);
    }
    ui.top_level(holder);
    f.full.then_some(fill)
}
/// Kung Fu: the combo's count from two hits up, right of the crosshair (returned: it pops).
fn combo_ui(ui: &mut Ui, show: &Show) -> Option<Entity> {
    let n = show.fight.as_ref().map_or(0, |f| f.combo);
    if n < 2 {
        return None;
    }
    let strike = ui.icon(Icon::Strike, 22., rgb(GOLD));
    let count = ui.text(&format!("{n}"), 30., rgb(GOLD));
    let row = ui.row(4., &[strike, count]);
    ui.world.entity_mut(row).insert(UiTransform::default());
    ui.top_level(row);
    Some(row)
}
/// Kung Fu: a blow blocked, a parry, a broken guard, over the crosshair (returned: it fades).
fn feel_ui(ui: &mut Ui, icon: Icon, tint: [f32; 3]) -> Entity {
    let e = ui.icon(icon, 26., rgb(tint));
    ui.top_level(e);
    e
}
fn lighten(c: [f32; 3], by: f32) -> [f32; 3] {
    c.map(|v| v + (1. - v) * by)
}

fn arms_ui(ui: &mut Ui, show: &Show, flash: bool, refused: bool) {
    let Some(arms) = &show.arms else { return };
    if !arms.others.is_empty() {
        let mut small = vec![];
        for (w, count) in &arms.others {
            let gun = ui.icon(Icon::Gun(*w), 16., rgba(DIM, 0.85));
            let mut kids = vec![gun];
            if let Some(n) = count {
                kids.push(ui.text(&format!("{n}"), 14., rgb(DIM)));
            }
            small.push(ui.row(4., &kids));
        }
        let row = ui.row(12., &small);
        ui.top_level(row);
    }
    let mut main = vec![];
    match arms.held {
        Held::Gun(w) => {
            let gun = ui.icon(Icon::Gun(w), 32., if arms.locked { rgba(WHITE, 0.5) } else { rgb(WHITE) });
            if arms.locked {
                let lock = ui.icon(Icon::Lock, 20., if refused { rgb(RED) } else { rgb(AMBER) });
                main.push(lock);
            }
            main.push(gun);
        }
        Held::Carry => main.push(ui.icon(Icon::Hand, 30., rgb(WHITE))),
        Held::Bacon => main.push(ui.icon(Icon::Bacon, 32., rgb(BACON))),
    }
    match arms.rounds {
        Some(Rounds::Count { now, full, reserve }) => {
            // The magazine low (a quarter or less) is amber and empty red; the reserve, beside it, red when there is none.
            let low = now <= (full / 4).max(1);
            let colour = if flash {
                MINT
            } else if now == 0 {
                RED
            } else if low {
                AMBER
            } else {
                WHITE
            };
            let bullet = ui.icon(Icon::Bullet, 22., rgb(colour));
            let count = ui.text(&format!("{now}"), 30., rgb(colour));
            let of = ui.text(&format!("/{reserve}"), 16., rgb(if reserve == 0 { RED } else { DIM }));
            let counts = ui.world.spawn(Node { align_items: AlignItems::Baseline, column_gap: ui.z.px(2.), ..default() }).add_children(&[count, of]).id();
            let ammo = ui.row(4., &[bullet, counts]);
            main.push(ammo);
        }
        Some(Rounds::Unlimited) => {
            let bullet = ui.icon(Icon::Bullet, 22., rgb(WHITE));
            let infinite = ui.icon(Icon::Infinity, 26., rgb(WHITE));
            main.push(ui.row(4., &[bullet, infinite]));
        }
        None => {}
    }
    let row = ui.row(14., &main);
    ui.top_level(row);
    // A reload under way: a thin bar under the rounds that fills as it goes (reload.rs).
    if let Some(eighths) = arms.reload {
        let fill = ui.world.spawn((Node { width: ui.z.px(9. * eighths as f32), height: ui.z.px(4.), ..default() }, BackgroundColor(rgb(AMBER)))).id();
        let bar = ui.world.spawn((Node { width: ui.z.px(72.), height: ui.z.px(4.), ..default() }, BackgroundColor(rgba(DIM, 0.35)))).add_children(&[fill]).id();
        ui.top_level(bar);
    }
    // Rounds just walked over: the box's icon and +N, a moment.
    if let Some((_, n)) = arms.gained {
        let icon = ui.icon(Icon::AmmoBox, 18., rgb(MINT));
        let plus = ui.text(&format!("+{n}"), 18., rgb(MINT));
        let pop = ui.row(4., &[icon, plus]);
        ui.top_level(pop);
    }
}

fn top_ui(ui: &mut Ui, show: &Show) {
    let z = ui.z;
    let pips = |ui: &mut Ui, won: u32, of: u32, tint: [f32; 3], mine: bool| -> Entity {
        let mut dots = vec![];
        for k in 0..of {
            let on = k < won;
            dots.push(
                ui.world
                    .spawn((
                        Node { width: z.px(13.), height: z.px(13.), border: UiRect::all(z.px(2.)), border_radius: BorderRadius::MAX, ..default() },
                        BackgroundColor(if on { rgb(tint) } else { Color::srgba(0.02, 0.02, 0.03, 0.45) }),
                        BorderColor::all(if on { Color::srgba(0.02, 0.02, 0.03, 0.9) } else { rgba(tint, 0.95) }),
                    ))
                    .id(),
            );
        }
        let row = ui.row(5., &dots);
        // This player's team: a bar under its pips.
        let mark = ui.world.spawn((Node { width: Val::Percent(100.), height: z.px(3.), border_radius: BorderRadius::all(z.px(1.5)), ..default() }, BackgroundColor(if mine { rgb(tint) } else { Color::NONE }))).id();
        ui.world.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: z.px(4.), ..default() }).add_children(&[row, mark]).id()
    };
    let mut chips = vec![];
    for chip in &show.top.chips {
        let mut kids = vec![];
        if let Some(k) = chip.key {
            kids.push(ui.cap(k));
        }
        kids.push(ui.icon(chip.icon, 22., rgb(chip.tint)));
        if !chip.text.is_empty() {
            kids.push(ui.text(&chip.text, 21., rgb(WHITE)));
        }
        chips.push(ui.row(5., &kids));
    }
    let middle = ui.row(16., &chips);
    let content = match show.top.teams {
        Some((scores, rounds, team)) => {
            let left = pips(ui, scores[0], rounds, crate::forts::TEAM_RGB[0], team == Some(0));
            let right = pips(ui, scores[1], rounds, crate::forts::TEAM_RGB[1], team == Some(1));
            ui.row(18., &[left, middle, right])
        }
        None => middle,
    };
    if show.top.teams.is_none() && show.top.chips.is_empty() {
        return;
    }
    let plate = ui
        .world
        .spawn((
            Node { padding: UiRect::axes(z.px(14.), z.px(6.)), border_radius: BorderRadius::all(z.px(8.)), align_items: AlignItems::Center, min_height: z.px(36.), ..default() },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.42)),
        ))
        .add_child(content)
        .id();
    ui.top_level(plate);
}

/// The objective: its icon, an arrow turned towards it (returned, to turn every frame), and how far.
fn objective_ui(ui: &mut Ui, o: Objective) -> Entity {
    let icon = ui.icon(o.icon, 24., rgb(o.tint));
    let arrow = ui.icon(Icon::Arrow, 20., rgb(o.tint));
    ui.world.entity_mut(arrow).insert(UiTransform::from_rotation(Rot2::radians(o.bearing)));
    let far = ui.text(&format!("{:.1}m", o.distance), 16., rgb(WHITE));
    let row = ui.row(6., &[icon, arrow, far]);
    ui.top_level(row);
    arrow
}

fn feed_ui(ui: &mut Ui, feed: &[Kill]) {
    let z = ui.z;
    for k in feed {
        let mut kids = vec![];
        if let Some((icon, tint)) = k.killer {
            kids.push(ui.icon(icon, 21., rgb(tint)));
        }
        if let Some(how) = k.how {
            kids.push(ui.icon(how, if matches!(how, Icon::Gun(_)) { 17. } else { 20. }, rgb(WHITE)));
        }
        kids.push(ui.text(&k.victim, 14., rgb(lighten(k.tint, 0.3))));
        let row = ui.row(6., &kids);
        let line = ui
            .world
            .spawn((Node { padding: UiRect::axes(z.px(8.), z.px(3.)), border_radius: BorderRadius::all(z.px(5.)), ..default() }, BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.5))))
            .add_child(row)
            .id();
        ui.top_level(line);
    }
}

fn prompt_ui(ui: &mut Ui, prompts: &[Prompt]) {
    for p in prompts {
        let mut kids = vec![];
        if let Some(key) = ui.key(p.key) {
            kids.push(key);
        }
        kids.push(ui.icon(p.icon, 24., rgb(p.tint)));
        if !p.text.is_empty() {
            kids.push(ui.text(&p.text, 15., rgb(p.tint)));
        }
        let row = ui.row(5., &kids);
        ui.top_level(row);
    }
}

fn moment_ui(ui: &mut Ui, moment: Option<&Moment>) {
    let Some(m) = moment else { return };
    let words = ui.text(&m.words, 40., rgb(m.tint));
    ui.top_level(words);
    if let Some((key, icon, text)) = &m.under {
        let mut kids = vec![];
        if let Some(k) = ui.key(*key) {
            kids.push(k);
        }
        if let Some(icon) = icon {
            kids.push(ui.icon(*icon, 22., rgb(WHITE)));
        }
        if !text.is_empty() {
            kids.push(ui.text(text, 22., rgb(WHITE)));
        }
        let row = ui.row(6., &kids);
        ui.top_level(row);
    }
}

fn watch_ui(ui: &mut Ui, watching: Option<&(String, [f32; 3])>) {
    let Some((name, tint)) = watching else { return };
    let eye = ui.icon(Icon::Eye, 22., rgb(WHITE));
    let who = ui.text(name, 18., rgb(lighten(*tint, 0.3)));
    let next = ui.icon(Icon::MouseLeft, 20., rgba(WHITE, 0.8));
    let row = ui.row(8., &[eye, who, next]);
    ui.top_level(row);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// Every word on the display in `show`, besides names and numbers.
    fn words(show: &Show) -> Vec<String> {
        let mut out: Vec<String> = show.top.chips.iter().map(|c| c.text.clone()).chain(show.prompts.iter().map(|p| p.text.clone())).collect();
        if let Some(m) = &show.moment {
            out.push(m.words.clone());
        }
        out
    }
    fn game(rules: &str) -> App {
        let mut app = crate::arena::test_app(rules);
        app.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        app
    }

    #[test]
    fn the_siege_shows_health_the_stim_the_gun_and_its_rounds_and_the_clock_as_icons_and_numbers() {
        let app = game("");
        let g = app.world().resource::<Game>();
        let show = Show::of(g, &Context::default());
        assert!(show.up);
        assert_eq!(show.health, Some(100.));
        assert_eq!(show.abilities[0], Ability { icon: Icon::Stim, charge: Charge::Ready, tint: MINT });
        let arms = show.arms.as_ref().unwrap();
        assert_eq!(arms.held, Held::Gun(6), "the syrup rifle in hand");
        assert_eq!(arms.rounds, Some(Rounds::Count { now: g.ammo[6], full: feast::FULL[6], reserve: g.mag.reserve[6] }));
        assert_eq!(arms.others, vec![(1, Some(g.ammo[1] + g.mag.reserve[1]))], "and the mustard pistol beside it");
        assert_eq!((arms.reload, arms.gained), (None, None));
        assert!(!arms.locked);
        assert_eq!(show.top.chips.iter().map(|c| c.icon).collect::<Vec<_>>(), [Icon::Clock, Icon::Hotdog, Icon::Skull]);
        assert_eq!(show.top.chips[1].text, "5");
        assert!(show.top.chips[0].text.contains(':'));
        // Numbers only: no word anywhere but a moment's.
        for w in words(&show) {
            assert!(w.chars().all(|c| c.is_ascii_digit() || ":/.m".contains(c)), "{w:?}");
        }
    }

    #[test]
    fn free_for_all_shows_the_clock_the_kills_to_win_and_its_rounds_running_out() {
        let mut app = game("mode=ffa");
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        let g = app.world().resource::<Game>();
        let show = Show::of(g, &Context::default());
        assert_eq!(show.top.chips[0].icon, Icon::Clock);
        assert_eq!(show.top.chips[1], Chip::new(Icon::Skull, WHITE, format!("0/{}", kitchen_sim::rules::FFA_KILLS)));
        // (Free For All's rounds run out: a magazine and its spare rounds, kitchen-sim magazine.rs.)
        assert_eq!(show.arms.unwrap().rounds, Some(Rounds::Count { now: 60, full: 60, reserve: kitchen_sim::magazine::SPARE[6] }));
        // Dead with a respawn to come: the moment says so, with the seconds.
        let mut g = app.world_mut().resource_mut::<Game>();
        let me = g.me();
        if let Some(p) = g.roster.iter_mut().find(|p| p.id == me) {
            (p.alive, p.respawn) = (false, 3.2);
        }
        let g = app.world().resource::<Game>();
        let show = Show::of(g, &Context::default());
        assert_eq!(show.moment, Some(Moment { words: "YOU DIED".into(), tint: RED, under: Some((Key::None, Some(Icon::Clock), "3.2".into())) }));
        assert!(show.health.is_none() && show.arms.is_none(), "nothing of first person, dead");
    }

    #[test]
    fn at_the_table_and_in_the_fridge_nothing_is_up() {
        let app = game("mode=cards");
        assert!(!Show::of(app.world().resource::<Game>(), &Context::default()).up);
        let app = game("");
        assert!(!Show::of(app.world().resource::<Game>(), &Context { holding: true, ..default() }).up);
    }

    #[test]
    fn forts_shows_each_teams_rounds_as_pips_and_the_build_clock_and_the_bacon_with_its_way_and_distance() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/pillow-fort.json")).unwrap();
        let map = kitchen_sim::map::Map::parse(&text).unwrap();
        let mut app = crate::arena::test_app("");
        app.init_resource::<crate::forts::Scene>().add_systems(Update, crate::arena::tick);
        {
            let world = app.world_mut();
            world.resource_mut::<crate::level::StartMap>().0 = map.clone();
            let mut g = world.resource_mut::<Game>();
            g.feast = kitchen_sim::feast::Feast::with_map(map, false);
        }
        crate::arena::test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert_eq!(g.phase_name(), "Build");
        let show = Show::of(g, &Context::default());
        assert_eq!(show.top.teams, Some(([0, 0], kitchen_sim::forts::ROUNDS, Some(0))));
        assert_eq!(show.top.chips[0].icon, Icon::Hammer);
        assert_eq!(show.top.chips[1].icon, Icon::Check);
        assert_eq!(show.top.chips[1].key, Some("R"), "not ready: R is shown by the tick");
        assert!(show.arms.unwrap().locked, "the guns are locked behind the forcefield");
        // The fight: the bacon, the way to it and how far.
        let mut g = app.world_mut().resource_mut::<Game>();
        g.feast.set_build(0.);
        for _ in 0..4 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        let g = app.world().resource::<Game>();
        assert_eq!(g.phase_name(), "Fight");
        let show = Show::of(g, &Context::default());
        let o = show.top.objective.expect("the bacon to steal");
        assert_eq!(o.icon, Icon::Bacon);
        let bacon = Vec3::from_array(g.fort.as_ref().unwrap().bacon.p);
        let flat = Vec2::new(bacon.x - g.position.x, bacon.z - g.position.z).length();
        assert!((o.distance - flat).abs() < 1e-3);
    }

    #[test]
    fn the_way_to_the_objective_is_measured_clockwise_from_straight_ahead() {
        // Looking along -z (yaw 0): ahead is 0, to the right (+x) a quarter turn clockwise, behind half a turn.
        let at = Vec3::ZERO;
        let (a, d) = bearing(at, 0., Vec3::new(0., 0., -2.));
        assert!(a.abs() < 1e-5 && (d - 2.).abs() < 1e-5);
        assert!((bearing(at, 0., Vec3::X).0 - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((bearing(at, 0., Vec3::NEG_X).0 + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((bearing(at, 0., Vec3::Z).0.abs() - std::f32::consts::PI).abs() < 1e-5);
        // Turned a quarter to the left (yaw +pi/2 looks along -x): -x is ahead.
        assert!(bearing(at, std::f32::consts::FRAC_PI_2, Vec3::NEG_X).0.abs() < 1e-5);
    }

    #[test]
    fn the_feed_names_the_victim_and_shows_the_killer_and_the_gun_as_icons() {
        let mut roster = kitchen_sim::feast::Feast::new(false).players;
        roster[1].last_damage = feast::Damage { kind: 2, attacker: 1, ..default() };
        let line = kill(&roster[1], &roster, false, &HashMap::new());
        assert_eq!(line.killer, Some((Icon::Hotdog, feast::COLORS[0])));
        assert_eq!((line.how, line.victim.as_str()), (Some(Icon::Gun(2)), roster[1].name.as_str()));
        for (kind, killer, how) in [
            (feast::SYRUP, Some(Icon::Hotdog), Some(Icon::Gun(6))),
            (feast::MAYONNAISE, Some(Icon::Hotdog), Some(Icon::Gun(7))),
            (feast::MELEE, Some(Icon::Hotdog), Some(Icon::Strike)),
            (feast::BACKSTAB, Some(Icon::Hotdog), Some(Icon::Backstab)),
            // The ketchup sniper in the face: its own mark.
            (2 | feast::FACE_SHOT, Some(Icon::Hotdog), Some(Icon::FaceShot)),
            (feast::ZOMBIE, Some(Icon::Cat), Some(Icon::Bite)),
            (feast::TURRET, Some(Icon::Turret), None),
            (feast::PESTS, Some(Icon::Flies), None),
            (feast::HUNGER, None, Some(Icon::Eat)),
            (feast::FALL, None, Some(Icon::Fall)),
            // A hot griddle: a flame, and whoever switched it on (nobody when the hotdog did itself).
            (feast::BURN, Some(Icon::Hotdog), Some(Icon::Flame)),
            (feast::BURN, None, Some(Icon::Flame)),
        ] {
            let attacker = if killer == Some(Icon::Hotdog) { 1 } else { 0 };
            roster[1].last_damage = feast::Damage { kind, attacker, ..default() };
            let line = kill(&roster[1], &roster, false, &HashMap::new());
            assert_eq!((line.killer.map(|k| k.0), line.how), (killer, how), "kind {kind}");
        }
        // Forts: in the killer's team colour.
        roster[0].team = 1;
        roster[1].last_damage = feast::Damage { kind: 1, attacker: 1, ..default() };
        assert_eq!(kill(&roster[1], &roster, true, &HashMap::new()).killer, Some((Icon::Hotdog, crate::forts::TEAM_RGB[1])));
    }

    #[test]
    fn a_griddle_in_reach_of_the_dial_and_faced_puts_a_key_cap_and_a_power_symbol_by_the_crosshair() {
        use kitchen_sim::map::{Block, Side};
        let mut app = game("");
        let dial = {
            let mut g = app.world_mut().resource_mut::<Game>();
            // A griddle on the bundled counter, its dial on its +z side, at (0.1, 3.1 + 0.14).
            g.feast.level.map.blocks.push(Block { rect: [-0.15, 2.96, 0.35, 3.24], height: 0.06, style: "griddle".into(), facing: Some(Side::PlusZ) });
            let block = g.feast.level.map.blocks.len() - 1;
            g.feast.griddles.push(kitchen_sim::griddle::Griddle::new(block));
            Vec3::new(0.1, feast::FLOOR, 3.24)
        };
        let prompt = |app: &App| {
            let g = app.world().resource::<Game>();
            Show::of(g, &Context::default()).prompts.into_iter().find(|p| p.icon == Icon::Power)
        };
        let stand = |app: &mut App, at: Vec3, yaw: f32| {
            let mut g = app.world_mut().resource_mut::<Game>();
            (g.position, g.yaw) = (at, yaw);
        };
        // 0.15 m in front of the dial, looking at it (towards -z): E and a power symbol, white while the griddle is off.
        stand(&mut app, dial + Vec3::Z * 0.15, 0.);
        let p = prompt(&app).expect("a prompt");
        assert_eq!((p.key, p.tint, p.text.as_str()), (Key::Cap("E"), WHITE, ""));
        // Amber while it is on.
        app.world_mut().resource_mut::<Game>().feast.griddles[0].on = true;
        assert_eq!(prompt(&app).map(|p| p.tint), Some(AMBER));
        // Too far, or turned away: none.
        stand(&mut app, dial + Vec3::Z * 0.4, 0.);
        assert!(prompt(&app).is_none());
        stand(&mut app, dial + Vec3::Z * 0.15, std::f32::consts::PI);
        assert!(prompt(&app).is_none());
        // By touch there is no key cap.
        stand(&mut app, dial + Vec3::Z * 0.15, 0.);
        let g = app.world().resource::<Game>();
        let touch = Show::of(g, &Context { touch: true, ..default() });
        assert_eq!(touch.prompts.iter().find(|p| p.icon == Icon::Power).map(|p| p.key), Some(Key::None));
    }

    #[test]
    fn a_death_goes_in_the_feed_for_a_few_seconds_and_a_new_phase_is_a_moment_of_few_words() {
        let mut app = game("");
        let mut hud = Hud::default();
        let colours = HashMap::new();
        hud.hear(app.world().resource::<Game>(), 0., &colours);
        assert!(hud.feed.is_empty());
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.roster[2].alive = false;
            g.roster[2].last_damage = feast::Damage { kind: feast::ZOMBIE, ..default() };
        }
        hud.hear(app.world().resource::<Game>(), 1., &colours);
        assert_eq!(hud.feed.len(), 1);
        assert_eq!(hud.feed[0].killer.map(|k| k.0), Some(Icon::Cat));
        hud.hear(app.world().resource::<Game>(), 1. + FEED_TIME + 0.1, &colours);
        assert!(hud.feed.is_empty(), "gone after FEED_TIME");
        // This player's own death: a moment.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            let me = g.me();
            g.roster.iter_mut().filter(|p| p.id == me).for_each(|p| p.alive = false);
        }
        hud.hear(app.world().resource::<Game>(), 10., &colours);
        assert_eq!(hud.moment(10.5).map(|m| m.words), Some("YOU DIED".into()));
        assert_eq!(hud.moment(13.5), None);
        // Every moment's words are three at most.
        for w in ["BUILD", "STEAL THE BACON", "WAVE", "FREE FOR ALL", "INFECTED", "RUN HOME", "YOU DIED", "ROUND WON", "ROUND LOST", "VICTORY", "DEFEAT", "NO SURVIVORS", "YOU WIN", "DRAW"] {
            assert!(w.split(' ').count() <= 3 && w.is_ascii());
        }
    }

    #[test]
    fn the_display_is_built_with_icons_at_the_windows_size_and_hidden_out_of_a_round() {
        let mut app = game("");
        app.init_resource::<Icons>().init_resource::<Hud>();
        for mut window in app.world_mut().query::<&mut Window>().iter_mut(app.world_mut()) {
            window.resolution.set(1920., 1080.);
        }
        app.world_mut().run_system_once(update).unwrap();
        let root = app.world_mut().query_filtered::<(Entity, &Visibility), With<Root>>().single(app.world()).map(|(e, v)| (e, *v)).unwrap();
        assert_ne!(root.1, Visibility::Hidden);
        // The icons are drawn for the pixels they take: at 1080 (1.5 times 720) the heart is 36 pixels high.
        let heights: Vec<u32> = {
            let world = app.world_mut();
            let handles: Vec<Handle<Image>> = world.query::<&ImageNode>().iter(world).map(|i| i.image.clone()).collect();
            let images = world.resource::<Assets<Image>>();
            handles.iter().filter_map(|h| images.get(h)).map(|i| i.height()).collect()
        };
        assert!(heights.contains(&36), "{heights:?}");
        // No text of the display says more than three words: numbers, names and moments.
        let world = app.world_mut();
        let texts: Vec<String> = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .filter(|(e, _)| std::iter::successors(Some(*e), |e| world.get::<ChildOf>(*e).map(|c| c.parent())).any(|e| world.get::<Root>(e).is_some()))
            .map(|(_, t)| t.0.clone())
            .collect();
        assert!(!texts.is_empty() && texts.iter().all(|t| t.split_whitespace().count() <= 3), "{texts:?}");
        assert!(texts.iter().all(|t| t.chars().all(|c| c.is_ascii_digit() || ":/.m".contains(c))), "numbers only here: {texts:?}");
        // At the card table it is hidden.
        app.world_mut().resource_mut::<Game>().feast.set_rules(kitchen_sim::rules::Rules::parse("mode=cards"));
        app.world_mut().run_system_once(update).unwrap();
        let vis = *app.world_mut().query_filtered::<&Visibility, With<Root>>().single(app.world()).unwrap();
        assert_eq!(vis, Visibility::Hidden);
    }

    // ----- Kung Fu.

    fn fighting() -> App {
        let mut app = game("mode=kungfu");
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        assert!(app.world().resource::<Game>().kung_fu(), "the round is Kung Fu's, and the client has its view");
        app
    }
    fn me_fighting(app: &mut App) -> Mut<'_, kitchen_sim::kungfu::FighterView> {
        let g = app.world_mut().resource_mut::<Game>();
        let me = g.me();
        Mut::map_unchanged(g, |g| g.kungfu.as_mut().unwrap().fighters.iter_mut().find(|f| f.id == me).unwrap())
    }

    #[test]
    fn kung_fu_shows_the_style_chi_and_kos_and_no_gun_no_rounds_and_no_stim() {
        let mut app = fighting();
        let g = app.world().resource::<Game>();
        let show = Show::of(g, &Context::default());
        assert!(show.up && show.health == Some(100.));
        assert!(show.arms.is_none(), "no gun, no rounds");
        assert!(show.abilities.iter().all(|a| a.icon != Icon::Stim), "no stim: its key dodges");
        let fight = show.fight.clone().expect("the fight's cluster");
        assert_eq!((fight.full, fight.iron, fight.combo, fight.key), (false, false, 0, Key::None));
        assert!(fight.style < 7 && fight.chi < 100.);
        assert_eq!(show.top.kos.map(|(mine, goal, _)| (mine, goal)), Some((0, kitchen_sim::kungfu::KOS)));
        assert_eq!(show.top.chips[0].icon, Icon::Clock);
        // Full chi glows and shows the special's key; a Toad's iron body rings its animal; a chain counts.
        {
            let mut f = me_fighting(&mut app);
            (f.chi, f.style, f.iron, f.combo, f.guard) = (100., 6, true, 4, 40.);
        }
        let g = app.world().resource::<Game>();
        let fight = Show::of(g, &Context::default()).fight.unwrap();
        assert_eq!((fight.full, fight.key, fight.style, fight.iron, fight.combo, fight.guard), (true, Key::Cap("T"), 6, true, 4, 40.));
        assert_eq!(Show::of(g, &Context { touch: true, ..default() }).fight.unwrap().key, Key::None, "by touch, no key cap");
        // The pulses: full chi glows between its colour and lighter, a combo's count pops and settles, feedback fades.
        assert!((0. ..=1.).contains(&glow_level(0.3)) && glow_level(0.) != glow_level(0.2));
        assert!(pop_scale(0.) > 1.4 && pop_scale(POP_TIME) == 1. && pop_scale(1.) == 1.);
        assert!(feel_alpha(0.) == 1. && feel_alpha(FEEL_TIME * 0.75) < 1. && feel_alpha(FEEL_TIME) == 0.);
        // Every style has its animal and its colour; none of them is a gun.
        assert_eq!(Icon::STYLES.len(), kitchen_sim::kungfu::STYLES);
        assert_eq!(STYLE_RGB.len(), kitchen_sim::kungfu::STYLES);
    }

    #[test]
    fn a_blow_blocked_a_parry_and_a_broken_guard_show_over_the_crosshair_and_a_new_style_says_its_name() {
        use kitchen_sim::kungfu::{Event, EventKind};
        let mut app = fighting();
        let mut hud = Hud::default();
        let colours = HashMap::new();
        let mut at = |app: &mut App, hud: &mut Hud, events: Vec<Event>, now: f32| {
            app.world_mut().resource_mut::<Game>().kungfu.as_mut().unwrap().events = events;
            hud.hear(app.world().resource::<Game>(), now, &colours);
        };
        at(&mut app, &mut hud, vec![], 1.);
        assert_eq!(hud.feel, None);
        let me = app.world().resource::<Game>().me();
        let e = |id: u32, kind: EventKind, by: u32, to: u32| Event { id, kind: Some(kind), by, to, ..default() };
        at(&mut app, &mut hud, vec![e(1, EventKind::Blocked, 9, me)], 2.);
        assert_eq!(hud.feel, Some((Icon::Guard, WHITE, 2.)), "this player blocked a blow");
        at(&mut app, &mut hud, vec![e(1, EventKind::Blocked, 9, me), e(2, EventKind::Parried, me, 9)], 3.);
        assert_eq!(hud.feel, Some((Icon::Parry, GOLD, 3.)), "it parried; the old event is not taken again");
        at(&mut app, &mut hud, vec![e(3, EventKind::Guardbroken, 9, me)], 4.);
        assert_eq!(hud.feel, Some((Icon::Broken, RED, 4.)), "its guard was broken");
        at(&mut app, &mut hud, vec![e(4, EventKind::Hit, 9, 8)], 5.);
        assert_eq!(hud.feel.map(|f| f.0), Some(Icon::Broken), "others' fights are not this player's to feel");
        // A new style: its name, for a moment, in its colour.
        me_fighting(&mut app).style = 1;
        at(&mut app, &mut hud, vec![], 6.);
        me_fighting(&mut app).style = 6;
        at(&mut app, &mut hud, vec![], 6.5);
        assert_eq!(hud.moment(6.6), Some(Moment { words: "TOAD".into(), tint: STYLE_RGB[6], under: None }));
        assert_eq!(hud.moment(8.), None);
        // A longer combo pops.
        me_fighting(&mut app).combo = 3;
        at(&mut app, &mut hud, vec![], 7.);
        assert_eq!(hud.popped, 7.);
    }

    #[test]
    fn the_kung_fu_display_is_built_without_a_gun_or_a_bullet_and_with_the_style_and_chi() {
        let mut app = fighting();
        app.init_resource::<Icons>().init_resource::<Hud>();
        {
            let mut f = me_fighting(&mut app);
            (f.style, f.combo) = (2, 5);
        }
        app.world_mut().run_system_once(update).unwrap();
        // Which icons the display holds: drawn by their size, the styles' at 44 (at 720), no gun's 2.6:1 box anywhere.
        let world = app.world_mut();
        let handles: Vec<Handle<Image>> = world.query::<&ImageNode>().iter(world).map(|i| i.image.clone()).collect();
        let icons = world.resource::<Icons>();
        let made: Vec<Icon> = handles.iter().filter_map(|h| icons.which(h)).collect();
        assert!(made.contains(&Icon::Style(2)) && made.contains(&Icon::Chi) && made.contains(&Icon::Strike), "{made:?}");
        assert!(!made.iter().any(|i| matches!(i, Icon::Gun(_) | Icon::Bullet | Icon::Infinity | Icon::Stim)), "{made:?}");
        // The combo's count is a number.
        let texts: Vec<String> = world.query::<&Text>().iter(world).map(|t| t.0.clone()).collect();
        assert!(texts.contains(&"5".to_string()), "{texts:?}");
    }
}

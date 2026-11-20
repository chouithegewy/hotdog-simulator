// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The event queue: what happened since the client last looked, gathered after every step (offline) and every message
//! (online). Things the simulation numbers itself (shots, splats, knocks, Kung Fu's events) are taken once each by their
//! number, as the Bevy client takes them; the rest (a hit, a death, a spawn, the stage, the winner, a griddle flipped, a
//! fighter's move beginning) are found by comparing the round with what it was. Each carries the sound the Bevy client
//! plays for it (src/fu.rs for Kung Fu, src/arena.rs and src/loose.rs for the rest).
use crate::session::{Game, QUEUE};
use kitchen_sim::{feast, kungfu, net};
use crate::*;
use kitchen_sim::feast::{Damage, Feast, Knock, Shot, Splat};
use kitchen_sim::kungfu::{EventKind, FighterView, Guard, Height, Kind, Move};
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

/// What the events were last worked out from.
#[derive(Default)]
pub struct Watch {
    pub round: u32,
    /// The newest shot, splat, knock and Kung Fu event taken.
    pub shot: u32,
    pub splat: u32,
    pub knock: u32,
    pub fu: u32,
    /// Each hotdog: alive, and its damage record.
    pub players: Vec<(u32, bool, Damage)>,
    pub stage: Option<u32>,
    pub winner: Option<u32>,
    /// Kung Fu: each fighter's move and when it began; this client's chi full, its style.
    pub moves: HashMap<u32, (u8, u32)>,
    pub full: bool,
    pub style: Option<u8>,
    /// Each griddle's block and whether it was on.
    pub griddles: Vec<(usize, bool)>,
}

pub fn push(q: &mut VecDeque<KsEvent>, e: KsEvent) {
    while q.len() >= QUEUE {
        q.pop_front();
    }
    q.push_back(e);
}

// ----- Sounds

/// The files under assets/ the Bevy client plays, by KS_SOUND_*.
pub fn sound_file(sound: u32) -> &'static str {
    match sound {
        KS_SOUND_PEW => "audio/pew.wav",
        KS_SOUND_SQUIRT => "audio/squirt.wav",
        KS_SOUND_BOOM => "audio/boom.wav",
        KS_SOUND_BITE => "audio/bite.wav",
        KS_SOUND_DING => "audio/ding.wav",
        KS_SOUND_CLICK => "audio/click.wav",
        KS_SOUND_FU_WHOOSH => "audio/fu-whoosh.wav",
        KS_SOUND_FU_WHOOSH_HEAVY => "audio/fu-whoosh-heavy.wav",
        KS_SOUND_FU_HIT => "audio/fu-hit.wav",
        KS_SOUND_FU_HIT_HEAVY => "audio/fu-hit-heavy.wav",
        KS_SOUND_FU_BLOCK => "audio/fu-block.wav",
        KS_SOUND_FU_PARRY => "audio/fu-parry.wav",
        KS_SOUND_FU_GUARDBREAK => "audio/fu-guardbreak.wav",
        KS_SOUND_FU_GRAB => "audio/fu-grab.wav",
        KS_SOUND_FU_THROW => "audio/fu-throw.wav",
        KS_SOUND_FU_GONG => "audio/fu-gong.wav",
        KS_SOUND_FU_CHI => "audio/fu-chi.wav",
        KS_SOUND_FU_STYLE => "audio/fu-style.wav",
        KS_SOUND_FU_CLANG => "audio/fu-clang.wav",
        _ => "",
    }
}
/// How loud the Bevy client plays a sound, at the listener (fu.rs `Sound::volume`; 1 for the rest).
fn volume(sound: u32) -> f32 {
    match sound {
        KS_SOUND_NONE => 0.,
        KS_SOUND_FU_WHOOSH => 0.45,
        KS_SOUND_FU_WHOOSH_HEAVY => 0.5,
        KS_SOUND_FU_HIT => 0.6,
        KS_SOUND_FU_HIT_HEAVY => 0.65,
        KS_SOUND_FU_BLOCK => 0.55,
        KS_SOUND_FU_PARRY => 0.6,
        KS_SOUND_FU_GUARDBREAK => 0.65,
        KS_SOUND_FU_GRAB => 0.5,
        KS_SOUND_FU_THROW => 0.6,
        KS_SOUND_FU_GONG => 0.55,
        KS_SOUND_FU_CHI => 0.45,
        KS_SOUND_FU_STYLE => 0.4,
        KS_SOUND_FU_CLANG => 0.55,
        _ => 1.,
    }
}
fn sounded(mut e: KsEvent, sound: u32) -> KsEvent {
    (e.sound, e.volume) = (sound, volume(sound));
    e
}
/// A blow this many HP or more is a heavy hit (fu.rs HEAVY).
const HEAVY: f32 = 12.;
/// What a move starting sounds like (fu.rs `swing`).
fn swing(mv: Move) -> u32 {
    match mv.data().kind {
        Kind::Light | Kind::Kick | Kind::Dodge => KS_SOUND_FU_WHOOSH,
        Kind::Heavy | Kind::Jump | Kind::Sweep | Kind::Special => KS_SOUND_FU_WHOOSH_HEAVY,
        _ => KS_SOUND_NONE,
    }
}

// ----- Events from the round's own lists

fn shot(s: &Shot) -> KsEvent {
    let beam = s.projectile == 0 && s.weapon == 0;
    let sound = if s.weapon == 1 { KS_SOUND_SQUIRT } else { KS_SOUND_PEW };
    sounded(KsEvent { kind: KS_EVENT_SHOT, time: s.time, id: s.id, by: s.owner, weapon: s.weapon as u32, value: s.projectile as i32, flags: if beam { KS_EVENT_FLAG_BEAM } else { 0 }, p: s.a, q: s.b, ..Default::default() }, sound)
}
fn splat(s: &Splat) -> KsEvent {
    let sound = if s.weapon == 4 { KS_SOUND_BOOM } else { KS_SOUND_NONE };
    sounded(KsEvent { kind: KS_EVENT_SPLAT, time: s.time, id: s.id, weapon: s.weapon as u32, value: s.projectile as i32, p: s.p, ..Default::default() }, sound)
}
fn knock(k: &Knock) -> KsEvent {
    let mut e = sounded(KsEvent { kind: KS_EVENT_KNOCK, time: k.time, id: k.id, to: k.prop, value: k.other, amount: k.speed, p: k.p, ..Default::default() }, KS_SOUND_BOOM);
    e.volume = (k.speed / 2.).clamp(0.15, 1.);
    e
}
/// Takes the items numbered past `mark` (numbers that began again, a new round's, start the mark again).
fn take<T>(q: &mut VecDeque<KsEvent>, items: &[T], number: impl Fn(&T) -> u32, mark: &mut u32, make: impl Fn(&T) -> KsEvent) {
    let newest = items.iter().map(&number).max().unwrap_or(0);
    if newest < *mark {
        *mark = 0;
    }
    let mut new: Vec<&T> = items.iter().filter(|x| number(x) > *mark).collect();
    new.sort_by_key(|x| number(x));
    for x in new {
        push(q, make(x));
    }
    *mark = (*mark).max(newest);
}

/// After an offline step: the round's shots, splats and knocks since the last, then what changed.
pub fn offline(g: &mut Game) {
    let Game { feast: Some(f), watch: w, events: q, .. } = g else {
        return;
    };
    if f.round != w.round {
        push(q, KsEvent { kind: KS_EVENT_ROUND, time: f.elapsed, value: f.round as i32, ..Default::default() });
        (w.round, w.shot, w.splat, w.knock) = (f.round, 0, 0, 0);
    }
    take(q, &f.shots, |s| s.id, &mut w.shot, shot);
    take(q, &f.splats, |s| s.id, &mut w.splat, splat);
    take(q, &f.knocks, |k| k.id, &mut w.knock, knock);
    changes(f, w, q, stage_index(f.stage));
    griddles(g);
    kungfu(g);
}
/// After a frame: its shots, splats and knocks (each sent once), then what changed.
pub fn online(g: &mut Game) {
    let Game { feast: Some(f), watch: w, events: q, online: Some(o), .. } = g else {
        return;
    };
    let Some(frame) = o.latest.as_ref() else {
        return;
    };
    if f.round != w.round {
        if w.round != 0 {
            push(q, KsEvent { kind: KS_EVENT_ROUND, time: f.elapsed, value: f.round as i32, ..Default::default() });
        }
        w.round = f.round;
    }
    for s in &frame.shots {
        push(q, shot(s));
    }
    for s in &frame.splats {
        push(q, splat(s));
    }
    for k in &frame.knocks {
        push(q, knock(k));
    }
    let stage = net::STAGES.iter().position(|s| *s == frame.stage).map_or(stage_index(f.stage), |i| i as u32);
    changes(f, w, q, stage);
}

/// Continuous harm is told by the HP, not as events.
fn continuous(kind: u8) -> bool {
    matches!(kind, feast::HUNGER | feast::PESTS | feast::BURN)
}
/// What changed: hits, deaths and spawns; the stage; the winner.
fn changes(f: &Feast, w: &mut Watch, q: &mut VecDeque<KsEvent>, stage: u32) {
    let now = f.elapsed;
    let mut seen = Vec::with_capacity(f.players.len());
    for p in &f.players {
        let d = p.last_damage;
        match w.players.iter().find(|x| x.0 == p.id) {
            Some(&(_, alive, record)) => {
                // (A round in the face is the gun's kind, flagged.)
                let (kind, flags) = (feast::plain_kind(d.kind), if feast::faced(d.kind) { KS_EVENT_FLAG_FACE } else { 0 });
                if d != record && d.kind != 0 && d.amount > 0. && !continuous(kind) {
                    let sound = if matches!(kind, feast::MELEE | feast::BACKSTAB) { KS_SOUND_BITE } else { KS_SOUND_NONE };
                    push(q, sounded(KsEvent { kind: KS_EVENT_DAMAGE, time: d.time, by: d.attacker, to: p.id, weapon: kind as u32, flags, amount: d.amount, p: d.p, q: d.d, ..Default::default() }, sound));
                }
                if alive && !p.alive {
                    push(q, KsEvent { kind: KS_EVENT_KILL, time: now, by: d.attacker, to: p.id, weapon: kind as u32, flags, amount: d.amount, p: p.p, q: d.d, ..Default::default() });
                }
                if !alive && p.alive {
                    push(q, KsEvent { kind: KS_EVENT_SPAWN, time: now, to: p.id, p: p.p, ..Default::default() });
                }
            }
            None if p.alive => push(q, KsEvent { kind: KS_EVENT_SPAWN, time: now, to: p.id, p: p.p, ..Default::default() }),
            None => {}
        }
        seen.push((p.id, p.alive, d));
    }
    w.players = seen;
    if w.stage != Some(stage) {
        push(q, KsEvent { kind: KS_EVENT_STAGE, time: now, value: stage as i32, ..Default::default() });
        w.stage = Some(stage);
    }
    if f.winner != w.winner {
        if let Some(id) = f.winner {
            push(q, KsEvent { kind: KS_EVENT_WIN, time: now, by: id, ..Default::default() });
        }
        w.winner = f.winner;
    }
}
/// A griddle switched on or off.
pub fn griddles(g: &mut Game) {
    let Game { feast: Some(f), watch: w, events: q, .. } = g else {
        return;
    };
    let now: Vec<(usize, bool)> = f.griddles.iter().map(|g| (g.block, g.on)).collect();
    for (gr, (block, on)) in f.griddles.iter().zip(&now) {
        if w.griddles.iter().find(|(b, _)| b == block).is_some_and(|(_, was)| was != on) {
            let e = KsEvent { kind: KS_EVENT_GRIDDLE, time: f.elapsed, by: gr.by, value: *block as i32, flags: if *on { KS_EVENT_FLAG_ON } else { 0 }, amount: gr.temp, ..Default::default() };
            push(q, sounded(e, KS_SOUND_CLICK));
        }
    }
    w.griddles = now;
}
/// Kung Fu (fu.rs `hear`): moves that began, the fight's events not taken yet, this client's chi filling and style
/// changing.
pub fn kungfu(g: &mut Game) {
    let me = g.me;
    let clock = g.feast.as_ref().map_or(0., |f| f.elapsed);
    let Game { kungfu, watch: w, events: q, feast, .. } = g;
    let Some(view) = kungfu.as_ref() else {
        w.moves.clear();
        (w.full, w.style) = (false, None);
        return;
    };
    let clock = clock.max(view.elapsed);
    let place = |id: u32| feast.as_ref().and_then(|f| f.player(id)).map_or([0.; 3], |p| p.p);
    for f in &view.fighters {
        let key = (f.mv, f.started.to_bits());
        let was = w.moves.insert(f.id, key);
        if was.is_some_and(|w| w != key) && f.t(clock) < 0.25 {
            let sound = swing(f.mv());
            push(q, sounded(KsEvent { kind: KS_EVENT_FU_MOVE, time: f.started, by: f.id, weapon: f.mv as u32, p: place(f.id), ..Default::default() }, sound));
        }
    }
    w.moves.retain(|id, _| view.fighters.iter().any(|f| f.id == *id));
    let last = view.events.iter().map(|e| e.id).max().unwrap_or(0);
    if last < w.fu {
        w.fu = 0;
    }
    let mut new: Vec<&kungfu::Event> = view.events.iter().filter(|e| e.id > w.fu).collect();
    new.sort_by_key(|e| e.id);
    for e in new {
        let Some(kind) = e.kind else {
            continue;
        };
        let (k, sound) = match kind {
            EventKind::Hit if e.mv == Move::Grab as u8 => (KS_EVENT_FU_HIT, KS_SOUND_FU_THROW),
            EventKind::Hit if e.amount >= HEAVY => (KS_EVENT_FU_HIT, KS_SOUND_FU_HIT_HEAVY),
            EventKind::Hit => (KS_EVENT_FU_HIT, KS_SOUND_FU_HIT),
            EventKind::Blocked => (KS_EVENT_FU_BLOCKED, KS_SOUND_FU_BLOCK),
            EventKind::Parried => (KS_EVENT_FU_PARRIED, KS_SOUND_FU_PARRY),
            EventKind::Guardbroken => (KS_EVENT_FU_GUARDBROKEN, KS_SOUND_FU_GUARDBREAK),
            EventKind::Grabbed => (KS_EVENT_FU_GRABBED, KS_SOUND_FU_GRAB),
            EventKind::Knockdown => (KS_EVENT_FU_KNOCKDOWN, KS_SOUND_FU_THROW),
            EventKind::Ko => (KS_EVENT_FU_KO, KS_SOUND_FU_GONG),
            EventKind::Absorbed => (KS_EVENT_FU_ABSORBED, KS_SOUND_FU_CLANG),
            EventKind::Dodged => (KS_EVENT_FU_DODGED, KS_SOUND_NONE),
            EventKind::Special => (KS_EVENT_FU_SPECIAL, KS_SOUND_NONE),
        };
        let flags = if e.ring { KS_EVENT_FLAG_RING } else { 0 };
        push(q, sounded(KsEvent { kind: k, time: e.time, id: e.id, by: e.by, to: e.to, weapon: e.mv as u32, value: e.last as i32, flags, amount: e.amount, p: e.p, ..Default::default() }, sound));
    }
    w.fu = w.fu.max(last);
    if let Some(f) = view.fighter(me) {
        let full = f.chi >= kungfu::CHI_MAX - 1e-3;
        if full && !w.full {
            push(q, sounded(KsEvent { kind: KS_EVENT_FU_CHI_FULL, time: clock, by: me, amount: f.chi, ..Default::default() }, KS_SOUND_FU_CHI));
        }
        w.full = full;
        if w.style.is_some_and(|s| s != f.style) {
            push(q, sounded(KsEvent { kind: KS_EVENT_FU_STYLE, time: clock, by: me, value: f.style as i32, ..Default::default() }, KS_SOUND_FU_STYLE));
        }
        w.style = Some(f.style);
    }
}

// ----- Kung Fu for the interface

/// A fighter as the interface tells it, at the round's clock.
pub fn fighter(f: &FighterView, clock: f32, alive: bool) -> KsFighter {
    let t = f.t(clock);
    let d = f.mv().data();
    let phase = if f.len <= 0. {
        KS_PHASE_LOOP
    } else if t < d.startup * f.scale {
        KS_PHASE_STARTUP
    } else if t < (d.startup + d.active) * f.scale {
        KS_PHASE_ACTIVE
    } else if t < f.len {
        KS_PHASE_RECOVERY
    } else {
        KS_PHASE_OVER
    };
    let mut flags = 0;
    for (on, bit) in [
        (f.connected, KS_FIGHTER_CONNECTED),
        (f.stunned, KS_FIGHTER_STUNNED),
        (f.down, KS_FIGHTER_DOWN),
        (f.invulnerable, KS_FIGHTER_INVULNERABLE),
        (f.iron, KS_FIGHTER_IRON),
        (alive && kungfu::free(f, clock), KS_FIGHTER_FREE),
        (!alive, KS_FIGHTER_DEAD),
    ] {
        if on {
            flags |= bit;
        }
    }
    KsFighter {
        id: f.id,
        style: f.style as u32,
        mv: f.mv as u32,
        started: f.started,
        t,
        len: f.len,
        scale: f.scale,
        phase,
        flags,
        block: f.block as u32,
        chi: f.chi,
        guard: f.guard,
        combo: f.combo,
        partner: f.partner,
        pace: if alive { kungfu::pace(f) } else { 0. },
    }
}
/// Every move's identifier ("TigerLight1"), made once and kept for the life of the process.
fn keys() -> &'static [String] {
    static KEYS: OnceLock<Vec<String>> = OnceLock::new();
    KEYS.get_or_init(|| Move::ALL.iter().map(|m| format!("{m:?}")).collect())
}
/// One row of the move table.
pub fn move_row(mv: Move) -> KsMove {
    let d = mv.data();
    let mut chains = [0u32; 4];
    for (slot, c) in chains.iter_mut().zip(d.chains) {
        *slot = *c as u32;
    }
    KsMove {
        id: mv as u32,
        key: KsStr::of(&keys()[mv as usize]),
        name: KsStr::of(d.name),
        kind: match d.kind {
            Kind::Stance => KS_MOVE_STANCE,
            Kind::Block => KS_MOVE_BLOCK,
            Kind::Parry => KS_MOVE_PARRY,
            Kind::Dodge => KS_MOVE_DODGE,
            Kind::Reaction => KS_MOVE_REACTION,
            Kind::KnockDown => KS_MOVE_KNOCKDOWN,
            Kind::GetUp => KS_MOVE_GETUP,
            Kind::Grab => KS_MOVE_GRAB,
            Kind::Thrown => KS_MOVE_THROWN,
            Kind::Victory => KS_MOVE_VICTORY,
            Kind::Light => KS_MOVE_LIGHT,
            Kind::Heavy => KS_MOVE_HEAVY,
            Kind::Kick => KS_MOVE_KICK,
            Kind::Jump => KS_MOVE_JUMP,
            Kind::Sweep => KS_MOVE_SWEEP,
            Kind::Special => KS_MOVE_SPECIAL,
        },
        style: mv.style().map_or(-1, |s| s.index() as i32),
        slot: mv.slot().map_or(-1, |s| s as i32),
        startup: d.startup,
        active: d.active,
        recovery: d.recovery,
        total: d.total(),
        reach: d.reach,
        arc: d.arc,
        damage: d.damage,
        pushback: d.pushback,
        stun: d.stun,
        guard: match d.guard {
            Guard::Blockable => KS_GUARD_BLOCKABLE,
            Guard::GuardBreak => KS_GUARD_BREAK,
            Guard::Unblockable => KS_GUARD_UNBLOCKABLE,
            Guard::Grab => KS_GUARD_GRAB,
        },
        height: match d.height {
            Height::High => KS_HEIGHT_HIGH,
            Height::Mid => KS_HEIGHT_MID,
            Height::Low => KS_HEIGHT_LOW,
        },
        chi: d.chi,
        knockdown: d.knockdown as u32,
        advance: d.advance,
        hits: d.hits as u32,
        iframes: d.iframes,
        low: d.low as u32,
        armor: d.armor as u32,
        chains,
        chain_count: d.chains.len().min(4) as u32,
    }
}

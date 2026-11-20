// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The Mantis: counters. An upright, narrow, twitchy fighter whose forearms are held up beside its face with the wrists
//! hooked over (two forelegs ready to strike), elbows tucked in under them. It pecks with the hooks, traps an arm and
//! pecks over the top, chops, reaches out and yanks a fighter in, snaps both forelegs shut like a pincer, and stamps
//! and hooks with its feet while the hooks stay high. Its special snares a fighter in both hooks, pecks it three times
//! and heaves it back over its head; if it catches nothing it looks at its empty hooks.
//!
//! The helpers at the bottom are shared with monkey.rs and drunken.rs: `grip` and `kick_at` put a hand or a foot on a
//! spot in the body's own frame wherever a spin has turned the body, `lifted` and `settle` lift a spinning or lying
//! body off (or down onto) the counter, and `carried` is how far the simulation has carried the hotdog by a beat.
//! KF3's tests of all three styles are at the very bottom.
//!
//! A note that shaped these moves: the arms are rubber hose (`reach`), and a hand passing quickly between stretched and
//! bent makes the elbow whip round as the arm straightens, so arms that move fast are kept long all the way.
use super::*;

/// The fingers of a hook point down past the knuckles: the palm faces down and forward and the wrist is bent over.
const HOOKED: f32 = 1.3;
/// A hook pecking: bent right over (and at the most a little more, short of folding back on itself).
const PECK: f32 = 1.55;
/// Where the hooks are held in the stance (from each shoulder: out, up, forward): up beside the face, the left one
/// higher and further forward.
const GUARD: [Vec3; 2] = [v(-0.003, 0.075, 0.042), v(0.0, 0.064, 0.030)];
/// The elbows tucked in and down under the hooks.
const TUCK: Vec3 = v(-0.15, -1., 0.4);

/// The hand on `side` hooked over by `flex`, its elbow tucked.
fn hooked(p: Pose, side: usize, flex: f32) -> Pose {
    let at = p.arms[side].hand;
    p.arm(side, at, Hand::Hook).palm(side, v(0., -1., 0.6), flex).elbow(side, TUCK)
}
/// The hand on `side` at `at` (from its shoulder) hooked over by `flex`.
fn hook(p: Pose, side: usize, at: Vec3, flex: f32) -> Pose {
    hooked(p.arm(side, at, Hand::Hook), side, flex)
}
/// Both hands.
fn hooks(p: Pose, left: Vec3, right: Vec3, flex: f32) -> Pose {
    hook(hook(p, L, left, flex), R, right, flex)
}
/// The hand on `side` hooked by `flex` at `at` in the body's frame (out, up from the counter, forward): where a blow
/// lands. The body is set first.
fn hook_at(p: Pose, side: usize, at: Vec3, flex: f32) -> Pose {
    hooked(grip(p, side, at, Hand::Hook), side, flex)
}

/// The mantis stance: the weight sat back on the rear leg, the front foot light, the body tall and leaning in a little,
/// both hooks up beside the face.
pub fn stance() -> Pose {
    let p = Pose::REST
        .hips(0., -0.007, -0.003)
        .lean(0.12)
        .twist(-0.10)
        .foot(L, v(0.013, 0., 0.020))
        .foot(R, v(0.016, 0., -0.008))
        .knee(L, v(0.1, 0., 1.))
        .knee(R, v(0.2, 0., 1.))
        .face(Face::FOCUS);
    hooks(p, GUARD[L], GUARD[R], HOOKED)
}

/// The idle: still as a mantis on a stem, swaying a little, the hooks twitching now and then.
pub fn idle(s: &Pose) -> Anim {
    let twitch = hook(*s, L, GUARD[L] + v(-0.004, 0.006, 0.014), PECK).twist(-0.16);
    let settle = hook(*s, L, GUARD[L] + v(0., -0.002, -0.002), HOOKED).hips(0., -0.008, -0.004);
    let sway = s.lean(0.15).twist(-0.06);
    Anim::new("mantis idle", *s)
        .key(1.2, sway, Ease::Smooth)
        .key(1.32, twitch, Ease::Snap)
        .key(1.5, settle, Ease::Out)
        .key(2.3, hook(*s, R, GUARD[R] + v(0., 0.004, 0.010), PECK), Ease::Snap)
        .key(2.45, *s, Ease::Out)
        .key(3., *s, Ease::Smooth)
        .cycle(Cycle::Loop(2.8))
}

/// The walk: stalking, each foot lifted high and set down with care, the hooks never coming down and twitching as it
/// goes. Its clock is the stride's.
pub fn walk(s: &Pose) -> Anim {
    let step = |side: usize| {
        let foot = s.legs[side].foot;
        let p = s.hips(s.hips.x - sign(side) * 0.0015, s.hips.y + 0.003, s.hips.z).lean(0.10).twist(s.twist - sign(side) * 0.06);
        let p = p.kick(side, foot + v(0.001, 0.014, 0.010), v(0.1, 0.6, 1.), 0.5);
        hook(p, side, GUARD[side] + v(0., 0.004, 0.006), PECK)
    };
    let down = s.hips(s.hips.x, s.hips.y - 0.0015, s.hips.z).lean(0.14);
    Anim::new("mantis walk", down)
        .key(0.75, step(L), Ease::Smooth)
        .key(1.5, down, Ease::Smooth)
        .key(2.25, step(R), Ease::Smooth)
        .key(3., down, Ease::Smooth)
        .cycle(Cycle::Stride)
}

pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Mantis",
        stance: s,
        idle: Some(idle(&s)),
        walk: Some(walk(&s)),
        moves: vec![
            hook_hand(&s),
            trapping_hook(&s),
            mantis_chop(&s),
            claw_pull(&s),
            pincer(&s),
            stamp_kick(&s),
            hook_kick(&s),
            pounce(&s),
            reaping_sweep(&s),
            snare(&s),
        ],
    }
}

/// Light 1, Hook Hand (high): the front hook draws back to the cheek and darts out at the face, pecking.
fn hook_hand(s: &Pose) -> Anim {
    let wound = hook(*s, L, v(-0.010, 0.060, 0.016), HOOKED).twist(0.08).lean(0.08).hips(0., -0.008, -0.005);
    let body = s.twist(-0.45).lean(0.22).hips(0., -0.008, 0.004).face(Face::FIERCE);
    let hit = hook_at(body, L, v(-0.012, 0.146, 0.112), PECK);
    let peck = hook_at(body.lean(0.24), L, v(-0.012, 0.136, 0.106), PECK + 0.2);
    Anim::new("Hook Hand", *s)
        .key(0.7, wound, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.6, peck, Ease::Out)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(L))
}

/// Light 2, Trapping Hook (high): the front hook reaches out and traps, the rear hook rises over the top and pecks
/// down on the head while the trapping hook yanks back to the chest.
fn trapping_hook(s: &Pose) -> Anim {
    let reach = s.twist(-0.30).lean(0.10);
    let wound = hook_at(hook(reach, R, v(0.004, 0.104, 0.012), 0.9), L, v(0.004, 0.130, 0.100), 0.9);
    let body = s.twist(0.42).lean(0.30).hips(0., -0.008, 0.003).face(Face::FIERCE);
    let hit = hook_at(hook(body, L, v(-0.006, 0.020, 0.020), PECK), R, v(-0.010, 0.144, 0.114), PECK);
    let follow = hook_at(hit.lean(0.33), R, v(-0.010, 0.132, 0.108), PECK + 0.2);
    Anim::new("Trapping Hook", *s)
        .key(0.62, wound, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.6, follow, Ease::Out)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
}

/// Light 3, Mantis Chop (mid): the rear hook winds up high over the opposite shoulder while the front one traps, then
/// the forearm chops down across the fighter's chest and the trapping hook yanks back to the hip.
fn mantis_chop(s: &Pose) -> Anim {
    // The chopping hand leads with its edge: its palm turned in, so the hook never folds along the forearm.
    let edge = |p: Pose, flex: f32| p.palm(R, v(-1., -0.4, 0.2), flex).elbow(R, v(1., -0.2, 0.5));
    let wound = edge(hook(hook_at(s.twist(-0.40).lean(0.0).hips(0., -0.005, -0.004), L, v(0.004, 0.125, 0.095), HOOKED), R, v(-0.050, 0.088, 0.040), 0.6), 0.6);
    let body = s.twist(0.50).lean(0.32).hips(0., -0.010, 0.004).face(Face::KIAI);
    // (The trapping arm yanks straight down and back to the hip: kept long, it never whips through straightening.)
    let hit = edge(hook_at(hook(body, L, v(0.020, -0.036, -0.012), HOOKED), R, v(-0.010, 0.100, 0.112), 0.7), 0.7);
    let follow = edge(hook_at(hit.lean(0.36).twist(0.56).hips(0., -0.012, 0.004), R, v(-0.030, 0.086, 0.104), 0.9), 0.9);
    let back = s.lean(0.16).twist(0.0);
    Anim::new("Mantis Chop", *s)
        .key(0.6, wound, Ease::Out)
        .key(1., hit, Ease::In)
        .key(1.7, follow, Ease::Out)
        .key(2.4, back, Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
}

/// Heavy 1, Claw Pull (mid): coiled back with both hooks by its ears, it shoots both out at the fighter's neck, snags,
/// and yanks the fighter in, sitting back hard into its rear leg.
fn claw_pull(s: &Pose) -> Anim {
    let coil = hooks(*s, v(0.006, 0.058, -0.004), v(0.006, 0.058, -0.004), 1.0).lean(-0.12).hips(0., -0.012, -0.010).squash(0.07).face(Face::FIERCE);
    let body = s.lean(0.36).twist(0.).hips(0., -0.010, 0.006).squash(-0.07).face(Face::KIAI);
    let hit = hook_at(hook_at(body, L, v(0.004, 0.112, 0.124), 0.9), R, v(0.004, 0.112, 0.124), 0.9);
    let snag = hook_at(hook_at(body, L, v(0.004, 0.106, 0.118), PECK + 0.2), R, v(0.004, 0.106, 0.118), PECK + 0.2);
    let yank = hooks(*s, v(0.004, 0.010, 0.006), v(0.004, 0.010, 0.006), PECK).lean(-0.24).twist(0.).hips(0., -0.014, -0.012).squash(0.06).face(Face::FIERCE);
    Anim::new("Claw Pull", *s)
        .key(0.75, coil, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(2., snag, Ease::Out)
        .key(2.35, yank, Ease::Snap)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
}

/// Heavy 2, Mantis Pincer (mid, breaks a guard, knocks down): both forelegs spread wide and high, up on its toes, then
/// snap shut on either side of the fighter's neck as it lunges, ending with the wrists together, hooks down, praying;
/// it bobs forward once more as it rights itself, like a dipping toy bird.
fn pincer(s: &Pose) -> Anim {
    let open = hooks(*s, v(0.050, 0.090, 0.010), v(0.050, 0.090, 0.010), 0.8)
        .elbow(L, v(1., 0., 0.3))
        .elbow(R, v(1., 0., 0.3))
        .lean(-0.14)
        .twist(0.)
        .hips(0., 0.003, -0.002)
        .squash(-0.06)
        .face(Face::FIERCE);
    let body = s.lean(0.45).twist(0.).hips(0., -0.006, 0.006).squash(-0.04).face(Face::KIAI);
    let shut = hook_at(hook_at(body, L, v(0.006, 0.122, 0.118), HOOKED), R, v(0.006, 0.122, 0.118), HOOKED).elbow(L, v(1., 0., 0.)).elbow(R, v(1., 0., 0.));
    let body = body.lean(0.55).hips(0., -0.009, 0.006);
    let pray = hook_at(hook_at(body, L, v(-0.002, 0.104, 0.110), PECK + 0.2), R, v(-0.002, 0.104, 0.110), PECK + 0.2).elbow(L, v(1., 0., 0.)).elbow(R, v(1., 0., 0.));
    Anim::new("Mantis Pincer", *s)
        .key(0.8, open, Ease::Out)
        .key(1., shut, Ease::Snap)
        .key(1.8, pray, Ease::Out)
        .key(2.4, s.lean(0.02), Ease::Smooth)
        .key(2.7, s.lean(0.22), Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
}

/// Kick 1, Mantis Stamp Kick (low): the front knee comes up while the hooks stay high, and the foot stamps down and out
/// on the fighter's shin.
fn stamp_kick(s: &Pose) -> Anim {
    let lift = hooks(*s, GUARD[L] + v(0., 0.004, 0.004), GUARD[R] + v(0., 0.004, 0.006), HOOKED)
        .kick(L, v(0.012, 0.034, 0.028), v(0., 0.3, 1.), -0.3)
        .lean(-0.05)
        .twist(-0.2)
        .hips(0., -0.003, -0.006);
    let hit = lift.kick(L, v(0.010, 0.016, 0.086), v(0., 0.2, 1.), -0.5).lean(-0.20).hips(0., -0.006, -0.002).face(Face::FIERCE);
    let press = hit.kick(L, v(0.010, 0.010, 0.084), v(0., 0.2, 1.), -0.5);
    let draw = s.kick(L, v(0.012, 0.016, 0.030), v(0., 0.3, 1.), 0.).lean(0.0);
    Anim::new("Mantis Stamp Kick", *s)
        .key(0.8, lift, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.8, press, Ease::Out)
        .key(2.4, draw, Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(L))
}

/// Kick 2, Hook Kick (high): the body tips away over its front leg as the rear knee chambers up and out, the leg whips
/// round to the fighter's head and the heel hooks back across it. The hooks never come down.
fn hook_kick(s: &Pose) -> Anim {
    let high = |p: Pose| hooks(p, GUARD[L] + v(0., 0.006, 0.0), GUARD[R] + v(0., 0.008, 0.010), HOOKED);
    let chamber = high(*s)
        .foot(L, v(0.010, 0., 0.004))
        .kick(R, v(0.046, 0.060, 0.016), v(1., 0.6, 0.2), 0.3)
        .tilt(0.45)
        .twist(0.2)
        .lean(-0.05)
        .hips(-0.006, 0.0, 0.);
    let hit = chamber.kick(R, v(0.006, 0.108, 0.088), v(0.3, 1., 0.), 0.8).tilt(0.55).twist(0.35).lean(-0.12).face(Face::KIAI);
    let hooked = hit.kick(R, v(-0.016, 0.100, 0.070), v(0., 1., 0.), 0.8).twist(0.42);
    let down = high(*s).kick(R, v(0.024, 0.026, 0.004), v(0.5, 0.5, 0.5), 0.2).tilt(0.15).hips(-0.003, -0.006, -0.004);
    Anim::new("Hook Kick", *s)
        .key(0.8, chamber, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.8, hooked, Ease::Out)
        .key(2.4, down, Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(R))
}

/// The jump attack, Pouncing Mantis: in the air both forelegs spread wide and high, then strike down together on the
/// fighter below; it lands low with its hooks still out.
fn pounce(s: &Pose) -> Anim {
    let tuck = |p: Pose| p.kick(L, v(0.013, 0.014, 0.012), v(0., 0.5, 1.), 0.4).kick(R, v(0.014, 0.018, -0.004), v(0., 0.5, 1.), 0.4);
    let start = tuck(*s);
    let open = tuck(hooks(*s, v(0.050, 0.094, 0.014), v(0.050, 0.094, 0.014), 0.8)).elbow(L, v(1., 0., 0.3)).elbow(R, v(1., 0., 0.3)).lean(-0.10).face(Face::FIERCE);
    let body = tuck(s.lean(0.55)).face(Face::KIAI);
    let hit = hook_at(hook_at(body, L, v(0.006, 0.075, 0.112), PECK), R, v(0.006, 0.075, 0.112), PECK);
    let after = hook_at(hook_at(body.lean(0.58), L, v(0.006, 0.066, 0.106), PECK), R, v(0.006, 0.066, 0.106), PECK);
    let land = hooks(*s, v(-0.026, 0.026, 0.070), v(-0.026, 0.026, 0.070), HOOKED).lean(0.40).hips(0., -0.016, 0.).squash(0.08);
    Anim::new("Pouncing Mantis", start)
        .key(0.6, open, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(2., after, Ease::Out)
        .key(2.4, land, Ease::In)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
}

/// The sweep, Reaping Sweep (low, knocks down): it sinks onto its front leg and the rear leg scythes round along the
/// counter from behind to the front and across, the body turning with it and the hooks still held up at its face.
fn reaping_sweep(s: &Pose) -> Anim {
    let low = |p: Pose| hooks(p, GUARD[L] + v(0., -0.004, 0.0), GUARD[R], HOOKED).hips(0., -0.022, 0.).lean(0.25).foot(L, v(0.012, 0., 0.004)).squash(0.05);
    let wound = low(*s).kick(R, v(0.056, 0.003, -0.024), v(1., 0.2, 0.), 0.4).twist(-0.6);
    let hit = low(*s).kick(R, v(0.008, 0.006, 0.086), v(0.4, 0.4, 1.), 0.4).twist(0.15).face(Face::FIERCE);
    let across = low(*s).kick(R, v(-0.050, 0.004, 0.052), v(-0.5, 0.5, 1.), 0.4).twist(0.75);
    let draw = low(*s).kick(R, v(0.010, 0.004, 0.020), v(0., 0.5, 1.), 0.2).twist(0.30);
    Anim::new("Reaping Sweep", *s)
        .key(0.8, wound, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(2., across, Ease::Out)
        .key(2.5, draw, Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(R))
}

/// The special, Mantis Snare (a grab): both hooks shoot out and snare the fighter, peck it three times while it is
/// held, yank it in and heave it back over the mantis's head. Catching nothing, the hooks snap shut on the air and the
/// mantis peers at them, baffled.
fn snare(s: &Pose) -> Anim {
    let coil = hooks(*s, v(0.006, 0.058, -0.004), v(0.006, 0.058, -0.004), 1.0).lean(-0.15).hips(0., -0.012, -0.010).squash(0.06).face(Face::FIERCE);
    let body = s.lean(0.34).twist(0.).squash(-0.05).face(Face::KIAI);
    let catch = hook_at(hook_at(body, L, v(0.006, 0.116, 0.122), 0.9), R, v(0.006, 0.116, 0.122), 0.9);
    let held = hook_at(hook_at(body.squash(0.).face(Face::FIERCE), L, v(0.006, 0.110, 0.110), PECK + 0.2), R, v(0.006, 0.110, 0.110), PECK + 0.2);
    let peck = |side: usize| hook_at(held.twist(0.3 * sign(side)), side, v(-0.008, 0.140, 0.116), PECK + 0.2);
    let yank = hooks(*s, v(0.004, 0.016, 0.026), v(0.004, 0.016, 0.026), PECK)
        .elbow(L, v(0.4, -1., -0.6))
        .elbow(R, v(0.4, -1., -0.6))
        .lean(-0.30)
        .twist(0.)
        .hips(0., -0.014, -0.010)
        .squash(0.06);
    let heave = hooks(*s, v(-0.004, 0.110, -0.020), v(-0.004, 0.110, -0.020), PECK)
        .elbow(L, v(1., 0.5, 0.))
        .elbow(R, v(1., 0.5, 0.))
        .lean(-0.45)
        .twist(0.)
        .hips(0., -0.010, -0.012)
        .face(Face::KIAI);
    let snapped = hooks(*s, v(-0.040, 0.050, 0.060), v(-0.040, 0.050, 0.060), PECK + 0.2).lean(0.30).twist(0.).face(Face::DAZED);
    let baffled = hooks(*s, v(-0.022, 0.040, 0.034), v(-0.022, 0.040, 0.034), HOOKED).lean(0.18).tilt(0.18).twist(0.).face(Face { brow: 0.6, raise: 0.9, mouth: 0.15, eyes: 0.7, ..Face::CALM });
    let caught = Anim::new("Mantis Snare", *s)
        .key(0.6, coil, Ease::Out)
        .key(1., catch, Ease::Snap)
        .key(2., held, Ease::Out)
        .key(2.09, peck(R), Ease::Snap)
        .key(2.18, peck(L), Ease::Snap)
        .key(2.27, peck(R), Ease::Snap)
        .key(2.5, yank, Ease::In)
        .key(2.72, heave, Ease::Out)
        .key(3., *s, Ease::Smooth);
    let missed = Anim::new("Mantis Snare (missed)", *s)
        .key(0.6, coil, Ease::Out)
        .key(1., catch, Ease::Snap)
        .key(2., held, Ease::Out)
        .key(2.15, snapped, Ease::Snap)
        .key(2.55, baffled, Ease::Out)
        .key(2.75, baffled.tilt(-0.12), Ease::Smooth)
        .key(3., *s, Ease::Smooth);
    caught.miss(missed).strike(Limb::Hand(R)).trail(Limb::Hand(L)).heavy()
}

// ----- Helpers for KF3's styles (mantis.rs, monkey.rs, drunken.rs)

/// The shoulder on `side`, and the chest's turn, in the hotdog's own frame (x right, y up, -z ahead), with the body as
/// `p` holds it, its squash and spin included.
fn shoulder(p: &Pose, side: usize) -> (Vec3, Quat) {
    let torso = torso_rotation(p.lean, p.twist, p.tilt);
    let spin = spin_rotation(p.spin);
    let scale = squash_scale(p.squash, p.belly);
    let at = PIVOT + body_frame(p.hips) - torso * (scale * PIVOT);
    let joint = at + torso * (scale * Vec3::new(sign(side) * SHOULDER.x, SHOULDER.y, 0.));
    (CENTRE + spin * (joint - CENTRE), spin * torso)
}
/// `p` with the hand on `side` at `at` in the body's own frame (out from the middle, up from the counter, forward from
/// the middle), as `Pose::hand_at` does, but wherever the body's spin has put the shoulder too: a hand on the counter in
/// a roll, a cartwheel or a fall. The body (hips, lean, twist, tilt, squash, spin) is set before this.
pub(super) fn grip(p: Pose, side: usize, at: Vec3, shape: Hand) -> Pose {
    let (joint, chest) = shoulder(&p, side);
    p.arm(side, limb_frame(side, chest.inverse() * (limb_frame(side, at) - joint)), shape)
}
/// `p` with the foot on `side` off the counter at `at` in the body's own frame (out, up, forward), its knee towards
/// `knee` (likewise), wherever the body's spin has turned the leg: a kick from a cartwheel or a fall.
pub(super) fn kick_at(p: Pose, side: usize, at: Vec3, knee: Vec3, toe: f32) -> Pose {
    let back = spin_rotation(p.spin).inverse();
    let foot = CENTRE + back * (limb_frame(side, at) + Vec3::Y * ANKLE - CENTRE) - Vec3::Y * ANKLE;
    p.kick(side, limb_frame(side, foot), limb_frame(side, back * limb_frame(side, knee)), toe)
}
/// How far the simulation has carried a hotdog fighting in `style` through `mv` by `beat` (right, up, forward): its
/// whole carry spread evenly over the startup and the active window, as the Animator takes it off. A key written
/// where the hotdog stands at that beat is `.moved(carried(..))` into the frame of where the move began; the Animator
/// moves it straight back, so a spinning body is shown exactly as written.
pub(super) fn carried(style: fu::Style, mv: fu::Move, beat: f32) -> Vec3 {
    let d = mv.data();
    let secs = if beat < 1. { beat * d.startup } else { d.startup + (beat - 1.).min(1.) * d.active };
    sim::travel(style.index(), mv) * (secs / (d.startup + d.active).max(1e-3))
}
/// The lowest of the body's sausage and its elbows, wrists, knees and ankles in pose `p`, and which it is.
pub(super) fn lowest(p: &Pose) -> (f32, &'static str) {
    let s = solve(p);
    let (at, chest, scale) = s.torso_place();
    let k = s.skeleton();
    let body = [BOTTOM, TOP].map(|y| (at + chest * (scale * Vec3::Y * y)).y - RADIUS * scale.x).into_iter().fold(f32::MAX, f32::min);
    let parts = [("left wrist", k.wrist[L]), ("right wrist", k.wrist[R]), ("left ankle", k.ankle[L]), ("right ankle", k.ankle[R])];
    let parts = parts.into_iter().chain([("left knee", k.knee[L]), ("right knee", k.knee[R]), ("left elbow", k.elbow[L]), ("right elbow", k.elbow[R])]);
    parts.fold((body, "body"), |low, (name, p)| if p.y < low.0 { (p.y, name) } else { low })
}
/// The pose lifted (or lowered) as a whole until the lowest of it is `gap` off the counter: a body lying or rolling on
/// it.
pub(super) fn settle(p: Pose, gap: f32) -> Pose {
    let low = lowest(&p).0;
    lifted(p, v(0., gap - low, 0.))
}
/// The whole pose (spin included) moved `by` (right, up, forward) as it is after its spin: off the counter for a flip,
/// along it for a roll. (The hips and both feet move by the unspun equivalent.)
pub(super) fn lifted(mut p: Pose, by: Vec3) -> Pose {
    let d = spin_rotation(p.spin).inverse() * body_frame(by);
    p.hips += body_frame(d);
    for (i, leg) in p.legs.iter_mut().enumerate() {
        leg.foot += limb_frame(i, d);
    }
    p
}

/// KF3's own checks of its three styles (Mantis, Monkey, Drunken), on top of the framework's for every style
/// (tests.rs): each move in its slot under its table name, timed by the table at any Drunken stretch, finite, smooth
/// from the stance and back into it, never under the counter, and each style's silhouette.
#[cfg(test)]
mod tests {
    use super::*;
    use fu::{Kind, Move, Style as Sim};

    const STYLES: [Sim; 3] = [Sim::Mantis, Sim::Monkey, Sim::Drunken];
    /// The joints compared from frame to frame.
    fn points(s: &Solved) -> Vec<Vec3> {
        let k = s.skeleton();
        let mut v = vec![k.eye, k.head, k.pivot];
        for i in [L, R] {
            v.extend([k.shoulder[i], k.elbow[i], k.wrist[i], k.tip(Limb::Hand(i)), k.hip[i], k.knee[i], k.ankle[i], k.tip(Limb::Foot(i))]);
        }
        v
    }
    fn gap(a: &Solved, b: &Solved) -> f32 {
        points(a).iter().zip(points(b).iter()).map(|(p, q)| p.distance(*q)).fold(0., f32::max)
    }
    /// Every move of the three styles, with its animation.
    fn moves() -> Vec<(Sim, Move, &'static Anim)> {
        let lib = library();
        STYLES
            .iter()
            .flat_map(|st| fu::Slot::ALL.iter().map(move |slot| (*st, st.move_in(*slot))))
            .map(|(st, mv)| (st, mv, lib.anim(st.index(), mv).unwrap_or_else(|| panic!("no animation for {mv:?}"))))
            .collect()
    }

    #[test]
    fn every_move_of_the_three_styles_has_its_own_animation_in_its_slot() {
        let all = moves();
        assert_eq!(all.len(), 30);
        for (st, mv, anim) in &all {
            assert_eq!(mv.style(), Some(*st));
            assert_eq!(anim.name, mv.data().name, "{mv:?} is animated by the move the table calls {}", mv.data().name);
            assert!(anim.keys.len() >= 4 && anim.cycle == Cycle::Once, "{mv:?}");
            assert!(anim.strike.is_some(), "{mv:?} says what lands its blow");
        }
        for st in STYLES {
            let set = library().set(st.index());
            assert_eq!(set.name, st.name());
            assert!(set.common[Common::Idle as usize].name.starts_with(&st.name().to_lowercase()), "{} has its own idle", st.name());
            assert!(set.common[Common::Walk as usize].name.starts_with(&st.name().to_lowercase()), "{} has its own walk", st.name());
        }
    }

    #[test]
    fn durations_match_the_table_and_every_blow_lands_on_the_first_beat_of_its_window() {
        for (st, mv, anim) in moves() {
            let d = mv.data();
            // A Drunken strike is stretched afresh each time; the others always play at the table's own timing.
            let scales: &[f32] = if st == Sim::Drunken { &[fu::DRUNK[0], 1., fu::DRUNK[1]] } else { &[1.] };
            for &scale in scales {
                let t = sim::timing(mv, scale, 0.);
                assert!((t.total() - d.total() * scale).abs() < 1e-5, "{mv:?} at {scale}");
                assert!((t.time(1.) - d.startup * scale).abs() < 1e-5 && (t.time(2.) - (d.startup + d.active) * scale).abs() < 1e-5, "{mv:?}");
                assert!((anim.beat(d.total() * scale, t) - 3.).abs() < 1e-4, "{mv:?} ends with its move");
                let at_end = solve(&anim.sample(d.total() * scale + 0.01, t));
                assert!(gap(&at_end, &solve(&anim.pose_at(3.))) < 1e-5, "{mv:?} holds its last pose");
            }
            assert_eq!(anim.contact(), 1., "{mv:?}: the blow lands as the active window opens");
        }
    }

    #[test]
    fn every_pose_is_finite_and_above_the_counter() {
        let mut failures = vec![];
        for (st, mv, anim) in moves() {
            let anims: Vec<&Anim> = std::iter::once(anim).chain(anim.miss.as_deref()).collect();
            for a in anims {
                let mut worst: Option<(f32, f32, &str)> = None;
                for i in 0..=600 {
                    let beat = i as f32 * 3. / 600.;
                    // (As it is shown: where the simulation has carried the hotdog taken off.)
                    let pose = a.pose_at(beat).moved(-carried(st, mv, beat));
                    let s = solve(&pose);
                    for p in points(&s) {
                        assert!(p.is_finite(), "{} {mv:?} beat {beat}: not a number", st.name());
                    }
                    assert!(s.reach.iter().chain(s.stride.iter()).all(|r| r.is_finite() && *r >= 1. && *r <= STRETCH + 1e-3), "{mv:?} beat {beat}");
                    let (low, what) = lowest(&pose);
                    if low <= -0.004 && worst.is_none_or(|w| low < w.0) {
                        worst = Some((low, beat, what));
                    }
                }
                if let Some((low, beat, what)) = worst {
                    failures.push(format!("{} {}: the {what} goes {low:.4} through the counter at beat {beat}", st.name(), a.name));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        for st in STYLES {
            let idle = &library().set(st.index()).common[Common::Idle as usize];
            for i in 0..=300 {
                let p = idle.pose_at(i as f32 / 100.);
                assert!(points(&solve(&p)).iter().all(|p| p.is_finite()) && lowest(&p).0 > -0.004, "{} idle", st.name());
            }
        }
    }

    #[test]
    fn moves_start_from_the_stance_and_end_in_it_without_a_jump() {
        let mut failures = vec![];
        for (st, mv, anim) in moves() {
            let stance = library().set(st.index()).stance;
            // A jump attack starts in the air (the framework blends into it); everything else from the stance.
            if mv.data().kind != Kind::Jump {
                assert!(gap(&solve(&anim.pose_at(0.)), &solve(&stance)) < 1e-5, "{mv:?} starts from the stance");
            }
            // It ends in the stance, wherever the simulation has carried it (a whole turn or flip is no turn at all).
            let end = solve(&stance.moved(sim::travel(st.index(), mv)));
            assert!(gap(&solve(&anim.pose_at(3.)), &end) < 1e-4, "{mv:?} ends in the stance");
            if let Some(miss) = &anim.miss {
                assert!(gap(&solve(&miss.pose_at(2.)), &solve(&anim.pose_at(2.))) < 1e-5 && gap(&solve(&miss.pose_at(3.)), &end) < 1e-4, "{mv:?} misses smoothly");
            }
            // Played at the quickest a Drunken strike can be, 2000 frames a second, no joint moves 8 mm in a frame (16 m/s),
            // as it is shown: with what the simulation has carried the hotdog taken off.
            let scale = if st == Sim::Drunken { fu::DRUNK[0] } else { 1. };
            let t = sim::timing(mv, scale, 0.);
            let shown = |a: &Anim, time: f32| solve(&a.sample(time, t).moved(-sim::carried(sim::travel(st.index(), mv), t, time)));
            for a in std::iter::once(anim).chain(anim.miss.as_deref()) {
                let mut was = shown(a, 0.);
                let (mut time, mut worst) = (0., (0f32, 0f32));
                while time < t.total() {
                    time += 1. / 2000.;
                    let s = shown(a, time);
                    let step = gap(&was, &s);
                    if step > worst.0 {
                        worst = (step, time);
                    }
                    was = s;
                }
                if worst.0 >= 0.008 {
                    failures.push(format!("{} {}: a joint jumps {:.4} at {:.3} s (beat {:.2})", st.name(), a.name, worst.0, worst.1, t.beat(worst.1)));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        for st in STYLES {
            let walk = &library().set(st.index()).common[Common::Walk as usize];
            assert!(walk.cycle == Cycle::Stride && gap(&solve(&walk.pose_at(0.)), &solve(&walk.pose_at(3.))) < 1e-5, "{} walk loops", st.name());
            let idle = &library().set(st.index()).common[Common::Idle as usize];
            assert!(gap(&solve(&idle.pose_at(0.)), &solve(&idle.pose_at(3.))) < 1e-5, "{} idle loops", st.name());
            assert!(gap(&solve(&idle.pose_at(0.)), &solve(&library().set(st.index()).stance)) < 1e-5, "{} idle is its stance", st.name());
        }
    }

    #[test]
    fn the_three_styles_read_from_their_silhouettes() {
        let stance = |st: Sim| solve(&library().set(st.index()).stance).skeleton();
        let (mantis, monkey, drunken) = (stance(Sim::Mantis), stance(Sim::Monkey), stance(Sim::Drunken));
        // The mantis stands tall with both hooks held up beside its face, elbows in under them.
        assert!(mantis.head.y > monkey.head.y + 0.02 && mantis.head.y >= drunken.head.y, "the mantis is the tallest: {} {} {}", mantis.head.y, monkey.head.y, drunken.head.y);
        for i in [L, R] {
            assert!(mantis.wrist[i].y > 0.14 && mantis.wrist[i].z < -0.01, "the mantis's hooks are up in front of its face: {}", mantis.wrist[i]);
            assert!(mantis.elbow[i].y < mantis.wrist[i].y - 0.02 && mantis.elbow[i].x.abs() < mantis.shoulder[i].x.abs() + 0.01, "elbows tucked in under the hooks: {} {}", mantis.elbow[i], mantis.wrist[i]);
        }
        // The monkey crouches low, slung forward, one long arm down to the counter and a hand on its head.
        assert!(monkey.head.y < 0.15 && monkey.head.z < -0.04, "the monkey crouches, leaning forward");
        assert!(monkey.wrist[L].y < 0.025 && monkey.wrist[R].distance(monkey.head) < 0.03, "a knuckle down by the counter, a hand scratching its head: {} {} {}", monkey.wrist[L], monkey.wrist[R], monkey.head);
        // The drunkard is tipped over sideways with its cup held up at its mouth.
        assert!((drunken.head.x - drunken.pivot.x).abs() > 0.02, "the drunkard leans over");
        assert!(drunken.wrist[R].y > 0.13 && drunken.wrist[R].z < -0.02, "the cup is up at its mouth");
        assert_eq!(solve(&library().set(Sim::Drunken.index()).stance).hands[R], Hand::Cup);
        assert_eq!(solve(&library().set(Sim::Mantis.index()).stance).hands, [Hand::Hook; 2]);
    }
}

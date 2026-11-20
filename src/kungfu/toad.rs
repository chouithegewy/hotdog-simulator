// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Toad style (KF5): the iron body. A hotdog that fights like a toad squats: hips sunk deep, body short and fat and
//! hunched, the knees out over the toes, the palms low and wide in front of them, the eyes half shut, unbothered
//! (`stance`). Standing still it sits back on its palms, flat on the counter like a toad's front feet, and croaks
//! (`sit`, its idle): that is its iron body (the simulation's `IRON_SHARE`), and ordinary blows bounce off it (fx.rs
//! `sheen`, `ripple`, `recoil`). It walks on all fours and is the slowest style on its feet, and every one of its blows
//! is a whole-body affair: a long wind-up while the cheeks (rig.rs's cheek pouches) and the belly fill with air, a snap
//! that lets it all out at once (the cheeks empty on contact, the mouth bursts open), a long hit-stop, a hard jolt for
//! the first-person camera, and the ground shaking under the stomps and the slams (`Anim::quake`).
//!
//! The ten moves, in `fu::Slot` order (names from the simulation's table): Belly Bump, Double-Palm Shove, Puffed Chest
//! Slam (the light chain), Iron Belly Ram, Twin Palm Thunder (the heavies), Heavy Stomp, Quaking Stomp (the kicks),
//! Leaping Body Slam (in the air), Squatting Sweep (crouched) and Iron Toad Avalanche (the signature special).
use super::*;

// ----- The faces

/// Heavy-lidded and unbothered: the toad at rest, its iron body on.
const STONE: Face = Face { brow: -0.3, raise: -0.35, mouth: 0., eyes: -0.45, puff: 0. };
/// The cheeks full, holding it all in.
const SWELL: Face = Face { brow: -0.7, raise: -0.4, mouth: 0., eyes: -0.2, puff: 1. };
/// Everything let out at once: the shout as the blow lands.
const BURST: Face = Face { brow: -1., raise: -0.5, mouth: 1., eyes: -0.55, puff: 0. };
/// Bursting with it: the eyes squeezed, the cheeks full.
const STRAIN: Face = Face { brow: -1., raise: -0.6, mouth: 0., eyes: -0.8, puff: 1. };
/// Swelling up: the eyes popping, the cheeks full.
const GOGGLE: Face = Face { brow: -0.4, raise: 0.5, mouth: 0., eyes: 0.7, puff: 1. };
/// Letting the air out after a big one: pfff.
const PFF: Face = Face { brow: 0.3, raise: 0.1, mouth: 0.3, eyes: -0.6, puff: 0. };

// ----- The squat and its hands

/// The feet of the squat: out from the middle, and forward.
const FEET: (f32, f32) = (0.034, 0.006);
/// How far the hips sink in the stance.
const DEPTH: f32 = 0.018;
/// The knees point out over the toes.
const KNEES: Vec3 = v(1., 0.3, 0.45);

/// The wide frog squat: the hips `depth` down (and a little back), the body leant `lean` forward, the belly blown out
/// `belly` and the body squashed `squash`; the feet wide and flat, the knees out over them.
fn squat(depth: f32, lean: f32, belly: f32, squash: f32) -> Pose {
    feet(Pose::REST.hips(0., -depth, -0.003).lean(lean).squash(squash).belly(belly), FEET.0, FEET.1).knee(L, KNEES).knee(R, KNEES)
}
/// Both feet planted `out` from the middle and `forward`.
fn feet(p: Pose, out: f32, forward: f32) -> Pose {
    p.foot(L, v(out, 0., forward)).foot(R, v(out, 0., forward))
}
/// Both hands open at `at` (the body's frame: out, up, forward; after the hips, lean and squash are set), the palms
/// facing `palm` and the wrists flexed `flex`, the elbows towards `elbow`.
fn palms(mut p: Pose, at: Vec3, palm: Vec3, flex: f32, elbow: Vec3) -> Pose {
    for side in [L, R] {
        p = p.hand_at(side, at, Hand::Palm).palm(side, palm, flex).elbow(side, elbow);
    }
    p
}
/// Palms flat on the counter `out` from the middle and `forward`, the fingers ahead: a toad's front feet.
fn planted(p: Pose, out: f32, forward: f32) -> Pose {
    palms(p, v(out, 0.005, forward), v(0., -1., 0.), -1.1, v(1., 0.5, -0.4))
}
/// A palm strike with both hands at `at`: the palms forward and the fingers up, the elbows out.
fn shove(p: Pose, at: Vec3) -> Pose {
    palms(p, at, v(0., -1., 0.), -1.3, v(1., -0.4, 0.))
}
/// Both arms flung back behind the hips, wherever they are (a belly bump leads with the belly).
fn flung(p: Pose) -> Pose {
    palms(p, v(0.052, 0.062, p.hips.z - 0.03), v(1., 0.2, -0.3), -0.4, v(0.5, 0.3, -1.))
}
/// Both arms spread wide and high, palms forward: swelling up to full size.
fn spread(p: Pose, high: f32) -> Pose {
    palms(p, v(0.078, high, 0.004), v(0., -1., 0.3), -1.0, v(1., -0.6, -0.2))
}
/// Both palms drawn back to the sides of the belly, ready to drive out.
fn chambered(p: Pose, up: f32) -> Pose {
    palms(p, v(0.047, up, 0.), v(0., -1., 0.), -1.3, v(0.7, -0.3, -1.))
}

/// The Toad's stance: the deep, wide squat, the belly out, the eyes half shut, both palms low in front of the knees
/// and wide, facing its man. (Standing still it sits back on them, on the counter: `sit`, its idle.) The shared moves
/// (blocks, dodges, falls) are built on this one.
pub fn stance() -> Pose {
    palms(squat(DEPTH, 0.2, 0.85, 0.17), v(0.052, 0.03, 0.046), v(0., -1., 0.), -0.9, v(1., -0.1, -0.3)).face(STONE)
}
/// Sitting its ground, its iron body on: the stance with both palms flat on the counter, like a toad's front feet.
pub fn sit() -> Pose {
    planted(squat(DEPTH, 0.2, 0.85, 0.17), 0.048, 0.038).face(STONE)
}
/// The stance stepped `forward` (where a move that travels ends).
fn ahead(s: &Pose, forward: f32) -> Pose {
    s.moved(v(0., 0., forward))
}
/// How far forward the simulation has carried a Toad by `beat` of `mv` (a move that travels: its keys are written from
/// where it began).
fn carried(mv: fu::Move, beat: f32) -> f32 {
    let timing = sim::timing(mv, 1., 0.);
    sim::carried(sim::travel(fu::Style::Toad.index(), mv), timing, timing.time(beat)).z
}
/// The name the simulation's table gives `mv`.
fn name(mv: fu::Move) -> &'static str {
    mv.name()
}

// ----- The idle: the croak

/// Sitting its ground (`sit`): breathing into the belly, then a gulp of air into the cheeks and out again; a slow blink.
fn idle() -> Anim {
    let s = sit();
    let breathe = s.belly(s.belly + 0.18).squash(s.squash - 0.03).hips(0., -DEPTH + 0.0012, -0.003);
    let croak = s.belly(s.belly - 0.12).squash(s.squash + 0.02).hips(0., -DEPTH - 0.001, -0.003).face(Face { puff: 0.95, eyes: -0.65, ..STONE });
    let after = s.face(Face { puff: 0.1, ..STONE });
    let blink = s.face(Face { eyes: -1., ..STONE });
    Anim::new("toad idle", s)
        .key(1.1, breathe, Ease::Smooth)
        .key(1.55, croak, Ease::Out)
        .key(1.8, after, Ease::In)
        .key(2.4, s, Ease::Smooth)
        .key(2.55, blink, Ease::Smooth)
        .key(2.7, s, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .cycle(Cycle::Loop(2.9))
}

// ----- The walk: on all fours

/// The Toad walks like a toad: the right palm and the left foot reach forward together, then the left palm and the right
/// foot, the body rocking over onto the pair left planted. (Its clock is the stride's.)
fn walk(s: &Pose) -> Anim {
    let s = *s;
    let reach = |side: usize| {
        let other = 1 - side;
        let p = s.hips(0.003 * sign(other), -DEPTH + 0.002, -0.001).tilt(0.07 * -sign(other)).twist(0.1 * sign(side));
        let p = palms(p, v(0.052, 0.03, 0.046), v(0., -1., 0.), -0.9, v(1., -0.1, -0.3));
        p.hand_at(side, v(0.052, 0.05, 0.064), Hand::Palm).kick(other, v(FEET.0, 0.01, FEET.1 + 0.012), KNEES, 0.3).face(STONE)
    };
    let down = s.squash(s.squash + 0.02).hips(0., -DEPTH - 0.001, -0.003);
    Anim::new("toad walk", down)
        .key(0.75, reach(R), Ease::Smooth)
        .key(1.5, down, Ease::Smooth)
        .key(2.25, reach(L), Ease::Smooth)
        .key(3., down, Ease::Smooth)
        .cycle(Cycle::Stride)
}

// ----- The light chain

/// 75 Belly Bump: hunches over sucking its belly in, the air held in its cheeks and the arms thrown back, then blows it
/// all into the belly at once and bumps it out into its man with a step.
fn belly_bump(s: &Pose) -> Anim {
    let s = *s;
    // The belly sucked right in and the air held in the cheeks, hunched over, arms back...
    let early = flung(squat(0.02, 0.22, 0.45, 0.17).hips(0., -0.02, -0.006)).face(Face { puff: 0.6, ..SWELL });
    let rock = flung(squat(0.022, 0.3, 0.2, 0.18).hips(0., -0.022, -0.01)).face(Face { puff: 1., eyes: 0.2, ..SWELL });
    // ...then all of it blown into the belly at once, the hips thrown forward over the right foot, which steps, the
    // belly leading and the back arched.
    let bump = flung(squat(0.008, -0.16, 1.5, -0.08).hips(0., -0.008, 0.07)).foot(R, v(FEET.0, 0., 0.064)).face(Face { mouth: 0.55, ..BURST });
    let over = bump.lean(-0.2).hips(0., -0.009, 0.073).belly(1.4);
    let back = s.foot(R, v(FEET.0, 0., 0.04)).hips(0., -DEPTH, 0.02).belly(1.);
    Anim::new(name(fu::Move::ToadLight1), s)
        .key(0.4, early, Ease::Out)
        .key(0.72, rock, Ease::Out)
        .key(1., bump, Ease::Snap)
        .key(1.45, over, Ease::Out)
        .key(2.3, back, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Head)
        .hitstop(0.075)
        .shake(1.3)
}
/// 76 Double-Palm Shove: both palms gathered at the hips, then shoved out together at chest height.
fn double_palm(s: &Pose) -> Anim {
    let s = *s;
    let gather = chambered(squat(0.019, 0.04, 1.1, 0.16), 0.05).face(Face { puff: 0.65, ..SWELL });
    let early = gather.belly(0.95).face(Face { puff: 0.35, ..SWELL });
    let hit = shove(squat(0.013, 0.32, 0.75, 0.04).hips(0., -0.013, 0.008), v(0.02, 0.085, 0.09)).face(Face { mouth: 0.6, ..BURST });
    let push = shove(hit.lean(0.36).hips(0., -0.013, 0.011), v(0.02, 0.084, 0.096));
    Anim::new(name(fu::Move::ToadLight2), s)
        .key(0.4, early, Ease::Out)
        .key(0.7, gather, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.5, push, Ease::Out)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .hitstop(0.075)
        .shake(1.3)
}
/// 77 Puffed Chest Slam: swells up tall with the arms spread, then throws the puffed chest down on its man.
fn chest_slam(s: &Pose) -> Anim {
    let s = *s;
    let rising = spread(squat(0.012, -0.15, 1.15, 0.), 0.12).face(Face { puff: 0.6, ..GOGGLE });
    let swell = spread(squat(0.008, -0.32, 1.35, -0.06), 0.135).face(GOGGLE);
    let slam = flung(squat(0.019, 0.6, 1.05, 0.16).hips(0., -0.019, 0.012)).face(BURST);
    let sink = slam.lean(0.66).squash(0.19);
    let wobble = slam.lean(0.4).squash(0.08).belly(0.8).face(Face { mouth: 0.2, ..STONE });
    Anim::new(name(fu::Move::ToadLight3), s)
        .key(0.3, rising, Ease::In)
        .key(0.65, swell, Ease::Out)
        .key(1., slam, Ease::Snap)
        .key(1.35, sink, Ease::Out)
        .key(2.2, wobble, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Head)
        .trail(Limb::Hand(R))
        .hitstop(0.09)
        .shake(1.5)
        .quake(1., 0.35)
}

// ----- The heavies

/// 78 Iron Belly Ram (travels): coils low on its palms like a sprinter, trembling, then launches belly-first, the arms
/// flung back and the back foot left where it was, and lands in its squat with the palms slapping the counter.
fn belly_ram(s: &Pose) -> Anim {
    let s = *s;
    let mv = fu::Move::ToadHeavy1;
    let go = carried(mv, 3.);
    let coil = |beat: f32, tilt: f32, depth: f32| planted(squat(depth, 0.45, 1.2, 0.16).hips(0., -depth, -0.008 + carried(mv, beat)).tilt(tilt), 0.04, 0.046).face(STRAIN);
    let launch = flung(squat(0.012, 0.02, 1.35, -0.06).hips(0., -0.012, carried(mv, 1.) + 0.046)).face(BURST);
    let launch = launch.foot(L, v(0.03, 0., 0.)).foot(R, v(0.034, 0., carried(mv, 1.) + 0.05));
    let through = launch.hips(0., -0.013, carried(mv, 2.) + 0.04).lean(-0.04).belly(1.25);
    let land = ahead(&planted(squat(0.019, 0.3, 0.9, 0.17), 0.046, 0.04), go).face(Face { mouth: 0.3, brow: -0.6, ..STONE });
    Anim::new(name(mv), s)
        .key(0.4, coil(0.4, 0., 0.02), Ease::Out)
        .key(0.62, coil(0.62, 0.05, 0.022), Ease::Out)
        .key(0.76, coil(0.76, -0.05, 0.023), Ease::Smooth)
        .key(1., launch, Ease::Snap)
        .key(1.6, through, Ease::Out)
        .key(2.25, land, Ease::In)
        .key(3., ahead(&s, go), Ease::Smooth)
        .strike(Limb::Head)
        .trail(Limb::Hand(L))
        .heavy()
        .hitstop(0.13)
        .shake(1.4)
        .quake(2.25, 0.5)
}
/// 79 Twin Palm Thunder (breaks a guard, knocks down): sinks and draws both palms back, gathers them in a great circle
/// overhead while it swells to bursting, coils them at the belly trembling, and thunders them out together with a
/// stamp of both feet; holds it, then lets the air out.
fn twin_palm(s: &Pose) -> Anim {
    let s = *s;
    let sink = palms(squat(0.021, 0.08, 1., 0.17), v(0.054, 0.03, -0.004), v(0., -1., 0.), -0.4, v(0.8, -0.2, -1.)).face(Face { puff: 0.5, ..SWELL });
    let rise = spread(squat(0.01, -0.32, 1.4, -0.06), 0.15).face(GOGGLE);
    let coil = chambered(squat(0.021, -0.02, 1.45, 0.14), 0.07).face(STRAIN);
    let tremble = coil.tilt(0.035).hips(0.001, -0.022, 0.);
    let thunder = shove(squat(0.012, 0.36, 0.95, -0.06).hips(0., -0.012, 0.012), v(0.012, 0.092, 0.112)).face(BURST);
    let hold = shove(thunder.lean(0.4).hips(0., -0.012, 0.014), v(0.012, 0.09, 0.118));
    let pff = palms(squat(0.016, 0.28, 0.25, 0.06), v(0.04, 0.04, 0.05), v(0., -1., 0.3), -0.5, v(1., 0., -0.3)).face(PFF);
    Anim::new(name(fu::Move::ToadHeavy2), s)
        .key(0.25, sink, Ease::Out)
        .key(0.52, rise, Ease::Smooth)
        .key(0.72, coil, Ease::Out)
        .key(0.82, tremble, Ease::Linear)
        .key(1., thunder, Ease::Snap)
        .key(1.55, hold, Ease::Out)
        .key(2., hold, Ease::Linear)
        .key(2.5, pff, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
        .hitstop(0.15)
        .shake(1.6)
        .quake(1., 0.7)
}

// ----- The stomps

/// 80 Heavy Stomp (low): the weight over onto the left foot, the right knee hauled up and out, arms out for balance,
/// and down on the foot of the man in front.
fn heavy_stomp(s: &Pose) -> Anim {
    let s = *s;
    // (The raised leg stays bent, its knee up and out: it straightens only as it stamps.)
    let lift = squat(0.012, -0.05, 1.1, 0.02).hips(-0.01, -0.012, -0.002).tilt(0.2).kick(R, v(0.024, 0.038, 0.014), v(1., 1., 0.3), -0.3);
    let lift = palms(lift, v(0.09, 0.105, 0.02), v(0., -1., 0.3), -0.9, v(1., -0.4, -0.2)).face(Face { eyes: 0.3, puff: 0.8, ..SWELL });
    let high = lift.kick(R, v(0.022, 0.044, 0.014), v(1., 1., 0.4), -0.35).tilt(0.24).hips(-0.011, -0.01, -0.002);
    let stomp = squat(0.021, 0.24, 1., 0.17).hips(0., -0.021, 0.01).foot(R, v(0.02, 0., 0.078));
    let stomp = palms(stomp, v(0.056, 0.03, 0.03), v(0., -1., 0.3), -0.6, v(1., 0.3, -0.3)).face(Face { mouth: 0.7, ..BURST });
    let settle = stomp.squash(0.13).lean(0.2);
    let back = s.foot(R, v(0.03, 0., 0.05)).lean(0.18);
    Anim::new(name(fu::Move::ToadKick1), s)
        .key(0.42, lift, Ease::Out)
        .key(0.78, high, Ease::Out)
        .key(1., stomp, Ease::Snap)
        .key(1.4, settle, Ease::Out)
        .key(2.3, back, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Foot(R))
        .hitstop(0.1)
        .shake(1.4)
        .quake(1., 1.)
}
/// 81 Quaking Stomp (all round, knocks down): dips, hops up with both arms raised and the belly blown up, and comes
/// down on both feet at once in the deepest squat, the right one stamped ahead, both palms slapping the counter.
fn quaking_stomp(s: &Pose) -> Anim {
    let s = *s;
    let dip = planted(squat(0.021, 0.25, 1.05, 0.17), 0.048, 0.038).face(Face { puff: 0.6, ..SWELL });
    let tuck = |p: Pose| p.kick(L, v(0.036, 0.016, 0.004), v(1., 0.6, 0.3), 0.2).kick(R, v(0.036, 0.016, 0.004), v(1., 0.6, 0.3), 0.2);
    let hop = palms(tuck(squat(-0.012, -0.1, 1.35, -0.1)), v(0.066, 0.175, 0.004), v(0., -1., 0.2), -0.6, v(1., -0.6, -0.1)).face(GOGGLE);
    let slam = planted(feet(squat(0.024, 0.3, 1.25, 0.2), 0.046, FEET.1), 0.062, 0.03).foot(R, v(0.03, 0., 0.07)).face(BURST);
    let rebound = slam.squash(0.15).hips(0., -0.021, -0.003);
    Anim::new(name(fu::Move::ToadKick2), s)
        .key(0.35, dip, Ease::Out)
        .key(0.8, hop, Ease::Out)
        .key(1., slam, Ease::Snap)
        .key(1.35, rebound, Ease::Out)
        .key(2.2, s.squash(0.14).foot(R, v(0.034, 0., 0.04)).face(Face { mouth: 0.2, ..STONE }), Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Foot(R))
        .trail(Limb::Foot(L))
        .heavy()
        .hitstop(0.12)
        .shake(1.2)
        .quake(1., 1.6)
}

// ----- In the air, and crouched

/// 82 Leaping Body Slam (the jump attack): balls up in the air, spreads out like a starfish, and comes down flat on
/// its seat with the palms slapping the counter.
fn body_slam(s: &Pose) -> Anim {
    let s = *s;
    let ball = squat(-0.004, 0.1, 1.3, 0.1).kick(L, v(0.026, 0.04, 0.02), v(0.6, 0.6, 1.), 0.4).kick(R, v(0.026, 0.04, 0.02), v(0.6, 0.6, 1.), 0.4);
    let ball = palms(ball, v(0.03, 0.075, 0.042), v(-1., 0., 0.), -0.2, v(1., -0.3, 0.)).spin(-0.35, 0., 0.).face(SWELL);
    let star = squat(-0.008, -0.1, 1.4, -0.08).kick(L, v(0.058, 0.016, 0.004), v(1., 0.2, 0.2), 0.6).kick(R, v(0.058, 0.016, 0.004), v(1., 0.2, 0.2), 0.6);
    let star = palms(star, v(0.085, 0.115, 0.012), v(0., -1., 0.2), -0.3, v(1., -0.5, 0.)).spin(0.18, 0., 0.).face(GOGGLE);
    let slam = planted(feet(squat(0.025, 0.36, 1.3, 0.2).hips(0., -0.025, 0.02), 0.052, 0.026), 0.07, 0.05).face(BURST);
    let flat = slam.squash(0.2).belly(1.4);
    Anim::new(name(fu::Move::ToadJump), s)
        .key(0.4, ball, Ease::Out)
        .key(0.7, star, Ease::Out)
        .key(1., slam, Ease::Snap)
        .key(1.6, flat, Ease::Out)
        .key(2.3, s.squash(0.15), Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Head)
        .trail(Limb::Hand(L))
        .heavy()
        .hitstop(0.12)
        .shake(1.2)
        .quake(1., 1.4)
}
/// 83 Squatting Sweep (low, knocks down): down on its palms, the right leg cocked out wide behind and swung round the
/// front along the counter.
fn squat_sweep(s: &Pose) -> Anim {
    let s = *s;
    let low = planted(squat(0.024, 0.5, 1., 0.17), 0.036, 0.05);
    let cock = low.twist(-0.35).kick(R, v(0.056, 0.003, -0.026), v(1., 0.4, 0.), 0.).face(Face { puff: 0.6, ..SWELL });
    let wound = low.twist(-0.45).kick(R, v(0.058, 0.003, -0.04), v(1., 0.4, -0.2), 0.).face(STRAIN);
    let sweep = low.twist(0.35).tilt(-0.08).kick(R, v(0.012, 0.004, 0.075), v(0.6, 1., 0.5), 0.3).face(Face { mouth: 0.6, ..BURST });
    let through = low.twist(0.5).tilt(-0.06).kick(R, v(-0.03, 0.004, 0.06), v(0.2, 1., 0.6), 0.3);
    let tuck = low.kick(R, v(0.036, 0.006, 0.012), v(1., 0.6, 0.3), 0.1).face(STONE);
    Anim::new(name(fu::Move::ToadSweep), s)
        .key(0.35, cock, Ease::Out)
        .key(0.7, wound, Ease::Out)
        .key(1., sweep, Ease::Snap)
        .key(1.6, through, Ease::Out)
        .key(2.3, tuck, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Foot(R))
        .hitstop(0.09)
        .shake(1.3)
}

// ----- The signature special

/// 84 Iron Toad Avalanche (travels, unblockable, knocks down): swells to bursting with the arms spread and the eyes
/// popping, rumbles forward in two quaking steps, coils, and drives everything into its man, both palms and the belly,
/// twice; then lets the air out.
fn avalanche(s: &Pose) -> Anim {
    let s = *s;
    let mv = fu::Move::ToadSpecial;
    let c = |beat: f32| carried(mv, beat);
    let go = c(3.);
    let swell = spread(squat(0.006, -0.32, 1.5, -0.08).hips(0., -0.006, c(0.22)), 0.165).face(Face { eyes: 1., raise: 0.8, ..GOGGLE });
    let swell = feet(swell, FEET.0, FEET.1);
    let stepping = |beat: f32, side: usize| {
        let p = squat(0.018, 0.05, 1.5, 0.12).hips(0.004 * sign(side), -0.018, c(beat)).tilt(-0.06 * sign(side));
        palms(p, v(0.074, 0.12, 0.01 + c(beat)), v(0., -1., 0.2), -0.9, v(1., -0.5, -0.2)).face(STRAIN)
    };
    let step1 = stepping(0.45, R).foot(R, v(FEET.0, 0., c(0.45) + 0.024)).foot(L, v(FEET.0, 0., FEET.1));
    let step2 = stepping(0.7, L).foot(L, v(FEET.0, 0., c(0.7) + 0.024)).foot(R, v(FEET.0, 0., c(0.45) + 0.024));
    let coil = chambered(squat(0.022, -0.02, 1.5, 0.15).hips(0., -0.022, c(0.82)), 0.07).face(STRAIN);
    let coil = coil.foot(L, v(FEET.0, 0., c(0.7) + 0.024)).foot(R, v(FEET.0, 0., c(0.82) + 0.006));
    let hit = shove(squat(0.01, 0.32, 1.1, -0.06).hips(0., -0.01, c(1.) + 0.012), v(0.012, 0.092, c(1.) + 0.115)).face(BURST);
    let hit = hit.foot(L, v(0.034, 0., c(0.7) + 0.024)).foot(R, v(FEET.0, 0., c(1.) + 0.03));
    let again = shove(hit.hips(0., -0.009, c(1.5) + 0.02).lean(0.42).belly(1.3), v(0.01, 0.09, c(1.5) + 0.128));
    let hold = again.hips(0., -0.01, c(2.) + 0.016);
    let pff = ahead(&palms(squat(0.016, 0.3, 0.15, 0.05), v(0.04, 0.035, 0.05), v(0., -1., 0.3), -0.5, v(1., 0., -0.3)), go).face(PFF);
    Anim::new(name(mv), s)
        .key(0.22, swell, Ease::Smooth)
        .key(0.45, step1, Ease::Snap)
        .key(0.7, step2, Ease::Snap)
        .key(0.82, coil, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.5, again, Ease::Snap)
        .key(2., hold, Ease::Linear)
        .key(2.5, pff, Ease::Smooth)
        .key(3., ahead(&s, go), Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
        .hitstop(0.17)
        .shake(1.6)
        .quake(0.45, 0.55)
        .quake(0.7, 0.7)
        .quake(1., 1.8)
}

pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Toad",
        stance: s,
        idle: Some(idle()),
        walk: Some(walk(&s)),
        moves: vec![
            belly_bump(&s),
            double_palm(&s),
            chest_slam(&s),
            belly_ram(&s),
            twin_palm(&s),
            heavy_stomp(&s),
            quaking_stomp(&s),
            body_slam(&s),
            squat_sweep(&s),
            avalanche(&s),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOAD: usize = 6;
    /// The Toad's ten moves, in `Slot` order, from the simulation.
    fn moves() -> Vec<fu::Move> {
        fu::Move::ALL.iter().copied().filter(|m| m.style() == Some(fu::Style::Toad)).collect()
    }
    fn finite(s: &Solved) -> bool {
        let k = s.skeleton();
        let points = [k.pivot, k.eye, k.head].into_iter().chain(k.shoulder).chain(k.elbow).chain(k.wrist).chain(k.hip).chain(k.knee).chain(k.ankle).chain(k.foot.map(|q| q * Vec3::Z)).chain(k.hand.map(|q| q * Vec3::Y));
        let mut ok = true;
        for p in points {
            ok &= p.is_finite();
        }
        let f = s.face;
        ok && s.shift.is_finite() && s.squash.is_finite() && s.belly.is_finite() && [f.brow, f.raise, f.mouth, f.eyes, f.puff].iter().all(|x| x.is_finite())
    }
    /// Every joint of a solved pose, in the hotdog's frame.
    fn joints(s: &Solved) -> Vec<Vec3> {
        let k = s.skeleton();
        [k.pivot, k.eye, k.head].into_iter().chain(k.shoulder).chain(k.elbow).chain(k.wrist).chain(k.hip).chain(k.knee).chain(k.ankle).collect()
    }

    #[test]
    fn the_toad_is_the_seventh_style_and_every_one_of_its_move_ids_has_its_own_animation() {
        assert_eq!(fu::Style::Toad.index(), TOAD);
        let lib = library();
        assert_eq!(lib.set(TOAD).name, "Toad");
        let ids: Vec<u8> = moves().iter().map(|m| m.id() as u8).collect();
        assert_eq!(ids, (75..=84).collect::<Vec<u8>>());
        for (slot, mv) in moves().into_iter().enumerate() {
            let a = lib.anim(TOAD, mv).unwrap_or_else(|| panic!("{mv:?} has no animation"));
            assert_eq!(a.name, mv.name(), "slot {slot}");
            assert!(std::ptr::eq(a, &lib.set(TOAD).moves[slot]));
            // And whoever plays a Toad move (a Toad), it is the Toad's.
            assert!(std::ptr::eq(lib.anim(0, mv).unwrap(), a));
            assert_eq!(a.cycle, Cycle::Once);
            assert!(a.strike.is_some(), "{mv:?} lands with something");
        }
        assert_eq!(lib.set(TOAD).moves.len(), SLOTS);
    }

    #[test]
    fn each_move_lasts_as_long_as_the_simulation_says_and_lands_on_the_first_beat_of_its_active_window() {
        let lib = library();
        for mv in moves() {
            let d = mv.data();
            let a = lib.anim(TOAD, mv).unwrap();
            let timing = sim::timing(mv, 1., 0.);
            assert_eq!((timing.startup, timing.active, timing.recovery), (d.startup, d.active, d.recovery), "{mv:?}");
            assert!((timing.total() - (d.startup + d.active + d.recovery)).abs() < 1e-6);
            // Keys rise from 0 to 3, so the animation spans the move exactly; the blow lands as the active window opens.
            assert_eq!(a.keys.first().map(|k| k.at), Some(0.), "{mv:?}");
            assert_eq!(a.keys.last().map(|k| k.at), Some(3.), "{mv:?}");
            assert!(a.keys.windows(2).all(|w| w[1].at > w[0].at), "{mv:?} keys rise");
            assert_eq!(a.contact(), 1., "{mv:?}");
            assert!((timing.time(a.contact()) - d.startup).abs() < 1e-6);
            assert!((a.beat(timing.total(), timing) - 3.).abs() < 1e-4 && a.beat(0., timing) == 0.);
            // The hit-stop is long (a Toad hits immensely hard) but never eats the recovery.
            assert!(a.stop() >= STOP && a.stop() < d.recovery * 0.6, "{mv:?} stop {}", a.stop());
        }
    }

    #[test]
    fn no_pose_of_the_toad_is_ever_nan_from_a_hundred_samples_a_move() {
        let set = library().set(TOAD);
        for (i, a) in set.moves.iter().chain(set.common.iter()).enumerate() {
            let timing = moves().get(i).map_or(Timing::default(), |m| sim::timing(*m, 1., 0.));
            for k in 0..=100 {
                let t = timing.total() * k as f32 / 100.;
                let s = solve(&a.sample(t, timing));
                assert!(finite(&s), "{} at {t}", a.name);
                assert!(s.reach.iter().chain(s.stride.iter()).all(|r| r.is_finite() && *r >= 1. && *r <= STRETCH + 1e-3), "{} at {t}: {:?} {:?}", a.name, s.reach, s.stride);
            }
        }
    }

    #[test]
    fn every_move_starts_and_ends_in_the_stance_so_blending_in_and_out_is_seamless() {
        let set = library().set(TOAD);
        let rest = joints(&solve(&set.stance));
        for mv in moves() {
            let a = library().anim(TOAD, mv).unwrap();
            let go = sim::travel(TOAD, mv);
            let first = joints(&solve(&a.pose_at(0.)));
            let last = joints(&solve(&a.pose_at(3.).moved(-go)));
            for (j, ((r, f), l)) in rest.iter().zip(&first).zip(&last).enumerate() {
                assert!(r.distance(*f) < 1e-5, "{mv:?} starts off the stance at joint {j}");
                assert!(r.distance(*l) < 1e-5, "{mv:?} ends off the stance at joint {j}: {r} {l}");
            }
        }
        // The idle loops round: its end is its start.
        let idle = &set.common[Common::Idle as usize];
        assert!(matches!(idle.cycle, Cycle::Loop(_)));
        assert_eq!(idle.pose_at(0.), idle.pose_at(3.));
    }

    /// The largest step any joint takes in the world between two frames `dt` apart, and when, as a Toad stands for a
    /// moment, throws `mv` (landing it), and stands again.
    fn largest_step(mv: fu::Move, dt: f32) -> (f32, f32) {
        let gait = solve(&stance());
        let mut animator = Animator::default();
        let timing = sim::timing(mv, 1., 0.);
        let whole = sim::travel(TOAD, mv);
        let mut was: Option<Vec<Vec3>> = None;
        let mut worst = (0f32, 0f32);
        let steps = ((timing.total() + 0.6) / dt) as usize;
        for k in 0..steps {
            let clock = k as f32 * dt - 0.2;
            let in_move = (0. ..timing.total()).contains(&clock);
            let carried = if in_move { sim::carried(whole, timing, clock) } else if clock >= timing.total() { whole } else { Vec3::ZERO };
            let f = sim::Fighter {
                style: TOAD,
                action: in_move.then_some(mv),
                t: if in_move { clock } else { 0. },
                timing: if in_move { timing } else { Timing::default() },
                connected: in_move && clock > timing.startup,
                carried: if in_move { carried } else { Vec3::ZERO },
                ..sim::Fighter::default()
            };
            let frame = animator.update(&f, Locomotion::default(), &gait, dt);
            assert!(finite(&frame.solved), "{mv:?} at {clock}");
            // (Where the body is in the world: the simulation has carried its root on.)
            let world: Vec<Vec3> = joints(&frame.solved).into_iter().map(|p| p + body_frame(carried)).collect();
            if let Some(was) = &was {
                for (a, b) in was.iter().zip(&world) {
                    if a.distance(*b) > worst.0 {
                        worst = (a.distance(*b), clock);
                    }
                }
            }
            was = Some(world);
        }
        worst
    }

    #[test]
    fn the_toad_moves_continuously_from_stance_to_move_to_stance_with_no_jumps() {
        // Continuous: the biggest step between frames shrinks with the frame time (a jump would not), and even the
        // snap of a rubber-hose palm (the fastest thing a Toad does) never carries a joint 2 cm in a millisecond.
        for mv in moves() {
            let (coarse, at) = largest_step(mv, 1. / 240.);
            let (fine, at_fine) = largest_step(mv, 1. / 960.);
            assert!(fine < 0.45 * coarse + 1e-4, "{mv:?}: {fine} at {at_fine} vs {coarse} at {at}");
            assert!(fine < 0.02, "{mv:?}: {fine} m at {at_fine}");
        }
    }

    #[test]
    fn in_first_person_the_toads_palms_shove_out_into_the_middle_of_the_picture() {
        // At contact the striking palm is ahead of the eye and in the picture (85 degrees tall, 16:9).
        let half = (85f32.to_radians() * 0.5).tan();
        let stance = solve(&stance());
        for mv in moves() {
            let a = library().anim(TOAD, mv).unwrap();
            let Some(Limb::Hand(i)) = a.strike else { continue };
            let p = view::place(&solve(&a.pose_at(a.contact())), &stance);
            let tip = p.fore[i].1 + p.hand[i] * Vec3::NEG_Y * 0.008;
            assert!(tip.z < -0.03, "{mv:?}: the palm is not ahead of the eye ({tip})");
            let (x, y) = (tip.x / -tip.z, tip.y / -tip.z);
            assert!(y.abs() < half * 0.8 && x.abs() < half * 16. / 9. * 0.8, "{mv:?}: the palm is not well in the picture ({x:.2}, {y:.2})");
        }
    }

    #[test]
    fn the_toad_sits_low_and_wide_with_its_palms_on_the_counter_and_its_belly_out() {
        let s = sit();
        let idle = &library().set(TOAD).common[Common::Idle as usize];
        assert_eq!(idle.pose_at(0.), s, "standing still, it sits");
        let k = solve(&s).skeleton();
        let plain = solve(&shared::stance()).skeleton();
        // Lower than the plain stance, and wider at the feet and the hands than at the hips.
        assert!(k.head.y < plain.head.y - 0.008, "{} {}", k.head.y, plain.head.y);
        assert!(s.hips.y <= -0.014 && s.belly >= 0.5);
        for i in [L, R] {
            assert!(k.ankle[i].x.abs() > HIP.x * 2.2, "feet wide");
            assert!(k.knee[i].x.abs() > k.hip[i].x.abs() + 0.004, "knees out");
            assert!(k.wrist[i].y < 0.012, "palm {i} on the counter: {}", k.wrist[i].y);
            assert!(-k.wrist[i].z > 0.02, "palm {i} in front");
        }
    }

    #[test]
    fn every_big_blow_fills_the_cheeks_before_it_and_lets_it_all_out_as_it_lands() {
        for mv in moves() {
            let a = library().anim(TOAD, mv).unwrap();
            let before = (40..100).map(|k| a.pose_at(k as f32 / 100.).face.puff).fold(0f32, f32::max);
            let at = a.pose_at(1.).face;
            assert!(before > 0.55, "{mv:?} puffs up first ({before})");
            assert!(at.puff < 0.05 && at.mouth > 0.5, "{mv:?} bursts on contact");
            // The belly is blown out for it, too.
            let belly = (40..=100).map(|k| a.pose_at(k as f32 / 100.).belly).fold(0f32, f32::max);
            assert!(belly > stance().belly + 0.15, "{mv:?} belly {belly}");
        }
    }

    #[test]
    fn the_stomps_and_slams_shake_the_ground_and_every_blow_stands_still_longer_than_most() {
        let lib = library();
        for mv in [fu::Move::ToadKick1, fu::Move::ToadKick2, fu::Move::ToadJump, fu::Move::ToadHeavy2, fu::Move::ToadSpecial] {
            let a = lib.anim(TOAD, mv).unwrap();
            assert!(a.quakes.iter().any(|(at, strength)| *at == 1. && *strength > 0.5), "{mv:?}");
        }
        for mv in moves() {
            let a = lib.anim(TOAD, mv).unwrap();
            assert!(a.stop() > STOP, "{mv:?}");
            if matches!(mv.data().kind, fu::Kind::Heavy | fu::Kind::Special) {
                assert!(a.heavy && a.stop() > HEAVY_STOP, "{mv:?}");
            }
            assert!(a.quakes.iter().all(|(at, s)| (0. ..=3.).contains(at) && *s > 0.));
        }
    }

    #[test]
    fn the_striking_limb_or_body_is_out_in_front_on_contact() {
        // Where the blow lands: the striking limb's tip, from where the Toad stands as the blow lands (the move's own
        // carry taken off), is ahead of where it is in the stance, low for the low blows.
        let rest = solve(&stance()).skeleton();
        for mv in moves() {
            let a = library().anim(TOAD, mv).unwrap();
            let timing = sim::timing(mv, 1., 0.);
            let carried = sim::carried(sim::travel(TOAD, mv), timing, timing.startup);
            let k = solve(&a.pose_at(1.).moved(-carried)).skeleton();
            let limb = a.strike.unwrap();
            let (now, then) = (k.tip(limb), rest.tip(limb));
            let ahead = -(now.z - then.z);
            // (The Quaking Stomp shakes the ground all round it: it lands where it stands.)
            match limb {
                _ if mv.data().arc >= 3. => {}
                Limb::Head => assert!(ahead > 0.005 || k.pivot.z < rest.pivot.z - 0.005, "{mv:?} {ahead}"),
                _ => assert!(-now.z > 0.045, "{mv:?} lands {} ahead", -now.z),
            }
            if mv.data().height == fu::Height::Low {
                assert!(now.y < 0.03, "{mv:?} low: {}", now.y);
            }
        }
    }
}

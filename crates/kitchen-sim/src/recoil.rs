// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Recoil: how hard each gun kicks, by how heavy it is (WEIGHT, one table for the simulation and the client's
//! first-person kick), and the aim punch the simulation gives every shot so that it is the same for everyone, bots
//! included, and the same on every machine.
//!
//! A hotdog's `bloom` (radians) is how far its aim has been thrown off by its own shots. A shot is deflected by up to
//! its bloom when it leaves (`deflect`: mostly up, a little either side, by a draw from the round's number, so it is
//! deterministic), and then adds its gun's kick: PUNCH times the gun's WEIGHT, times how steadily it is held
//! (`steadiness`: half prone, three quarters crouched, half again aimed down the sights; and the tuning's `recoil`). It
//! settles in a straight line, a shot's worth in SETTLE seconds (by gun), and never passes MOST. The heavy guns kick
//! hardest but fire slowly enough to settle between shots at their quickest. The mustard pistol is a hand cannon: light
//! in the hand, but its muzzle flips (HAND_CANNON, not its weight's kick) and takes SETTLE[PISTOL] to come back down,
//! longer than it takes to fire again, so a shot fired as soon as it may goes where the muzzle still is, high and off
//! to a side (`deflect`: the pistol's mostly up), and fired as fast as it goes its kicks stack up to FLIP: patient
//! shots go true, spamming walks them up and over. The syrup rifle's burst keeps its own climb (feast.rs CLIMB): its
//! rounds after the first leave along the aim as it is then, and the burst's kicks have settled by the next.
//! The spray guns (the hot sauce thrower, the bug spray and the mayonnaise) shake steadily while their trigger is held
//! instead: each shot holds the bloom at its level (`kick`) rather than adding to it.
use crate::feast::{Player, GUNS, PISTOL};
use glam::Vec3;

/// How heavy each gun is, as a share of the syrup rifle (by weapon number; 0 the strike). The mustard pistol and the
/// mayonnaise bottle are the lightest, the ketchup sniper and the relish bazooka the heaviest.
pub const WEIGHT: [f32; GUNS] = [0., 0.45, 2.0, 1.3, 2.6, 0.55, 1.0, 0.5];
/// The guns that spray: a steady shake while they are fired, not a kick a shot.
pub const SPRAY: [bool; GUNS] = [false, false, false, true, false, true, false, true];
/// The bloom a shot adds for each unit of WEIGHT (radians), and the most bloom there is (but for the pistol's, FLIP).
pub const PUNCH: f32 = 0.006;
pub const MOST: f32 = 0.03;
/// The mustard pistol's kick, set as it is rather than by its weight: a hand cannon's, half as much again as a shot of
/// the ketchup sniper (0.012) and three syrup rounds' (0.006), and the most its kicks stack up to, five of them (about
/// 5°: fired on and on, its rounds go over a head a metre off).
pub const HAND_CANNON: f32 = 0.018;
pub const FLIP: f32 = 0.09;
/// Seconds one shot's kick takes to settle, by gun (the pistol's longer than the 0.35 s it takes to fire again).
pub const SETTLE: [f32; GUNS] = [1., 0.8, 0.8, 0.15, 0.9, 0.15, 0.095, 0.15];

/// The bloom one shot of gun `w` adds (a spray gun's: the level it holds), before how steadily it is held.
pub fn kick(w: usize) -> f32 {
    if w == PISTOL { HAND_CANNON } else { PUNCH * WEIGHT.get(w).copied().unwrap_or(0.) }
}
/// The most gun `w`'s kicks stack up to.
pub fn most(w: usize) -> f32 {
    if w == PISTOL { FLIP } else { MOST }
}
/// How much of a kick lands, by how the hotdog holds its gun: half prone, three quarters crouched, and half of that
/// aimed down the sights (`Player::aimed`).
pub fn steadiness(p: &Player) -> f32 {
    let stance = if p.prone { 0.5 } else if p.crouch { 0.75 } else { 1. };
    stance * if p.aimed { 0.5 } else { 1. }
}
/// How fast gun `w`'s bloom settles, radians a second.
pub fn settling(w: usize) -> f32 {
    kick(w) / SETTLE.get(w).copied().unwrap_or(1.).max(1e-3)
}
/// A hotdog's bloom after a shot of gun `w` (`scale` the tuning's recoil for it).
pub fn after_shot(p: &Player, w: usize, scale: f32) -> f32 {
    let k = kick(w) * steadiness(p) * scale.max(0.);
    if SPRAY.get(w).copied().unwrap_or(false) { p.bloom.max(k).min(MOST) } else { (p.bloom + k).min(most(w).max(p.bloom)) }
}
/// How far to a side the pistol's flipped muzzle sends a round, as a share of its bloom.
pub const PISTOL_SIDE: f32 = 0.4;
/// Two numbers from -1 to 1, the same on every machine for the same round of the same hotdog.
fn draw(owner: u32, round: u32) -> (f32, f32) {
    let mut z = (owner as u64) << 32 ^ round as u64 ^ 0x9E37_79B9_7F4A_7C15;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let unit = |bits: u64| (bits & 0xFF_FFFF) as f32 / 0xFF_FFFF as f32 * 2. - 1.;
    (unit(z), unit(z >> 24))
}
/// The way a round of gun `w` leaves when its hotdog's aim has bloomed by `bloom` radians: `dir` turned up by between
/// a quarter of it down and all of it up, and to the side by up to three quarters of it, by a draw from the hotdog and
/// the round's number; the pistol's muzzle, flipped up, sends it up by 0.7 to all of it and to the side by up to
/// PISTOL_SIDE of it. No bloom, no change.
pub fn deflect(dir: Vec3, bloom: f32, owner: u32, round: u32, w: usize) -> Vec3 {
    if bloom <= 0. {
        return dir;
    }
    let (a, b) = draw(owner, round);
    let side = dir.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = side.cross(dir).normalize_or(Vec3::Y);
    let (yaw, pitch) = if w == PISTOL { (bloom * PISTOL_SIDE * a, bloom * (0.85 + 0.15 * b)) } else { (bloom * 0.75 * a, bloom * (0.375 + 0.625 * b)) };
    (dir + side * yaw.tan() + up * pitch.tan()).normalize_or(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feast::{Feast, DELAY, MAYO, RIFLE, SNIPER, VOLLEY, VOLLEY_GAP};

    #[test]
    fn heavier_guns_kick_more_a_shot_and_the_pistol_is_the_lightest_the_sniper_and_bazooka_the_heaviest() {
        // (The pistol aside: a hand cannon, its kick is set as it is.)
        let mut by_weight: Vec<usize> = (2..GUNS).collect();
        by_weight.sort_by(|a, b| WEIGHT[*a].total_cmp(&WEIGHT[*b]));
        for pair in by_weight.windows(2) {
            assert!(kick(pair[0]) < kick(pair[1]), "{pair:?}");
        }
        let heaviest = by_weight[GUNS - 4..].to_vec();
        assert!(heaviest.contains(&2) && heaviest.contains(&4), "{heaviest:?}");
        assert!((1..GUNS).filter(|w| !SPRAY[*w]).all(|w| WEIGHT[1] <= WEIGHT[w]), "the pistol is the lightest gun that kicks");
        assert_eq!(WEIGHT[RIFLE], 1., "the syrup rifle is the reference");
    }

    #[test]
    fn the_mustard_pistol_is_a_hand_cannon_its_kick_by_the_snipers_its_muzzle_slow_to_come_down_and_stacking_to_a_cap() {
        assert!(kick(PISTOL) >= 2.5 * kick(RIFLE) && kick(PISTOL) > kick(SNIPER) && kick(PISTOL) < 2. * kick(SNIPER), "{}", kick(PISTOL));
        assert!(SETTLE[PISTOL] > DELAY[PISTOL] * 2., "it can fire again well before its muzzle is back");
        assert!(most(PISTOL) >= 5. * kick(PISTOL) - 1e-6 && most(RIFLE) == MOST);
        // Fired as fast as it goes it stacks, each shot leaving more, up to FLIP and no further.
        let (mut p, mut blooms) = (Player::default(), vec![]);
        for _ in 0..20 {
            p.bloom = after_shot(&p, PISTOL, 1.);
            blooms.push(p.bloom);
            p.bloom = (p.bloom - settling(PISTOL) * DELAY[PISTOL]).max(0.);
        }
        assert!(blooms[..5].windows(2).all(|b| b[1] > b[0] + 0.005) && blooms.iter().all(|b| *b <= FLIP + 1e-7) && (blooms[19] - FLIP).abs() < 1e-7, "{blooms:?}");
        // A round of it leaves mostly up: from 0.7 to all of the bloom up, and less than half of it to a side.
        let d = Vec3::new(0.6, 0., -0.8);
        for round in 0..200 {
            let e = deflect(d, 0.02, 3, round, PISTOL);
            let side = d.cross(Vec3::Y).normalize();
            let (up, across) = (e.y.asin(), e.dot(side).asin());
            assert!(up >= 0.7 * 0.02 - 1e-5 && up <= 0.02 + 1e-5 && across.abs() <= PISTOL_SIDE * 0.02 + 1e-5, "{up} {across}");
        }
    }

    #[test]
    fn the_heavy_guns_settle_between_shots_at_their_quickest_and_the_pistol_blooms_under_sustained_fire() {
        let mut p = Player::default();
        // One shot settles in SETTLE seconds: before its gun can fire again for the sniper, the bazooka and a syrup burst.
        for w in [2, 4] {
            p.bloom = 0.;
            p.bloom = after_shot(&p, w, 1.);
            assert!(p.bloom > 0.01 && p.bloom - settling(w) * DELAY[w] <= 0., "gun {w} settles in its {} s", DELAY[w]);
        }
        // A syrup burst: each round's kick has settled by the next, VOLLEY_GAP on, and the last's by the next burst.
        p.bloom = 0.;
        for _ in 0..VOLLEY {
            p.bloom = after_shot(&p, RIFLE, 1.);
            assert!(p.bloom - settling(RIFLE) * VOLLEY_GAP <= 0., "settled by the next round");
            p.bloom = (p.bloom - settling(RIFLE) * VOLLEY_GAP).max(0.);
        }
        assert!(kick(RIFLE) - settling(RIFLE) * (DELAY[RIFLE] - (VOLLEY - 1) as f32 * VOLLEY_GAP) <= 0., "a burst has settled by the next");
        // The pistol fired as fast as it goes: each shot leaves something, and it grows.
        let mut blooms = vec![];
        p.bloom = 0.;
        for _ in 0..4 {
            p.bloom = (after_shot(&p, 1, 1.) - settling(1) * DELAY[1]).max(0.);
            blooms.push(p.bloom);
        }
        assert!(blooms.windows(2).all(|b| b[1] > b[0]) && blooms[3] > 3. * blooms[0], "{blooms:?}");
        // A spray gun holds a steady level however long it is fired.
        p.bloom = 0.;
        for _ in 0..50 {
            p.bloom = after_shot(&p, MAYO, 1.);
        }
        assert_eq!(p.bloom, kick(MAYO));
    }

    #[test]
    fn crouching_prone_and_aiming_steady_the_kick() {
        let mut p = Player::default();
        let standing = after_shot(&p, 2, 1.);
        p.crouch = true;
        let crouched = after_shot(&p, 2, 1.);
        (p.crouch, p.prone) = (false, true);
        let prone = after_shot(&p, 2, 1.);
        p.aimed = true;
        let aimed = after_shot(&p, 2, 1.);
        assert!(standing > crouched && crouched > prone && prone > aimed && aimed > 0.);
        assert!((prone - standing * 0.5).abs() < 1e-7 && (aimed - standing * 0.25).abs() < 1e-7);
        assert_eq!(after_shot(&Player::default(), 2, 0.), 0., "a tuning of no recoil");
    }

    #[test]
    fn a_shot_is_deflected_by_its_bloom_the_same_way_every_time_and_mostly_up() {
        let d = Vec3::new(0.6, 0., -0.8);
        assert_eq!(deflect(d, 0., 3, 9, SNIPER), d);
        let (mut ups, mut sides) = (0, [0, 0]);
        for round in 0..400 {
            let e = deflect(d, 0.02, 3, round, SNIPER);
            assert_eq!(e, deflect(d, 0.02, 3, round, SNIPER), "deterministic");
            let angle = e.angle_between(d);
            assert!(angle <= 0.02 * 1.3 && (e.length() - 1.).abs() < 1e-5, "{angle}");
            ups += (e.y > 0.) as usize;
            sides[(e.cross(d).y > 0.) as usize] += 1;
        }
        assert!(ups > 300, "mostly up: {ups}");
        assert!(sides[0] > 120 && sides[1] > 120, "either side: {sides:?}");
    }

    #[test]
    fn in_the_simulation_the_first_shot_goes_true_the_next_too_soon_is_thrown_off_and_the_bloom_settles() {
        let mut s = Feast::new(false);
        s.begin_famine();
        let d = Vec3::new(0., 0., -1.);
        let p = s.player_mut(1).unwrap();
        (p.owned[2], p.ammo[2], p.weapon, p.gun) = (true, 6, 2, 2);
        assert!(s.fire(1, d, 2));
        assert!(Vec3::from_array(s.projectiles[0].v).normalize().angle_between(d) < 1e-6, "the first shot goes where it is aimed");
        let bloom = s.player(1).unwrap().bloom;
        assert!((bloom - kick(2)).abs() < 1e-7);
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(s.fire(1, d, 2));
        assert!(Vec3::from_array(s.projectiles[1].v).normalize().angle_between(d) > 1e-4, "thrown off by the kick");
        s.tick(1.7);
        assert_eq!(s.player(1).unwrap().bloom, 0., "settled, a shot's worth every SETTLE seconds");
    }

    #[test]
    fn the_pistol_fired_again_as_soon_as_it_may_goes_high_where_its_muzzle_is_and_a_patient_shot_goes_true() {
        let mut s = Feast::new(false);
        s.begin_famine();
        for p in &mut s.players {
            (p.bot, p.hp) = (false, 1e6);
        }
        let d = Vec3::new(0.6, 0., -0.8);
        let way = |s: &Feast| Vec3::from_array(s.projectiles.last().unwrap().v).normalize();
        let p = s.player_mut(1).unwrap();
        (p.owned[PISTOL], p.ammo[PISTOL], p.weapon, p.gun) = (true, 12, PISTOL, PISTOL);
        assert!(s.fire(1, d, PISTOL));
        assert!(way(&s).angle_between(d) < 1e-6, "a rested shot goes where it is aimed");
        // Again as soon as it may: the muzzle is still up, so the round goes up, by more than a rifle round's kick, and
        // a little to a side.
        while !s.fire(1, d, PISTOL) {
            s.tick(1. / 60.);
        }
        let quick = way(&s);
        assert!(quick.y.asin() > kick(RIFLE), "up {} rad", quick.y.asin());
        assert!(quick.angle_between(d) < s.player(1).unwrap().bloom, "no further than the muzzle has come up");
        // Fired on as fast as it goes, the shots walk up, to the cap and no further.
        let mut rises = vec![quick.y.asin()];
        for _ in 0..8 {
            while !s.fire(1, d, PISTOL) {
                s.tick(1. / 60.);
            }
            rises.push(way(&s).y.asin());
            assert!(s.player(1).unwrap().bloom <= FLIP + 1e-6);
        }
        assert!(rises[3] > 2.5 * rises[0] && rises.iter().all(|r| *r <= FLIP), "{rises:?}");
        // Patient: once the muzzle has come back down (SETTLE a shot's worth), the next goes true again.
        s.tick(FLIP / settling(PISTOL) + 0.05);
        assert_eq!(s.player(1).unwrap().bloom, 0.);
        assert!(s.fire(1, d, PISTOL));
        assert!(way(&s).angle_between(d) < 1e-6, "settled, it goes where it is aimed");
    }
}

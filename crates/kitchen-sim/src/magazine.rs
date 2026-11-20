// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Magazines, reserves and reloading, in every mode with guns.
//!
//! **A gun's rounds** are of two kinds: those in its magazine (`Player::ammo`, at most MAGAZINE, which is feast.rs FULL:
//! what the trigger takes from) and its reserve (`Player::reserve`, at most CARRY). R reloads the gun in hand
//! (`Feast::reload`): for RELOAD seconds (by gun, `tune.reload`) it cannot fire, and then as many rounds as the magazine
//! has room for go into it from the reserve. Taking another gun in hand, picking a loose object up, a melee strike or
//! death cancels a reload; a gun whose magazine is empty starts one when its trigger is pulled, if its reserve has
//! rounds. Out of rounds altogether a gun stays in hand, empty; the melee strike needs none.
//!
//! **Pickups**, the same in every gun mode whatever the map says (a map's `ammo` and `respawn` are read and passed
//! over): a gun's pickup gives PICKUP rounds (a magazine, but the power weapons fewer) and comes back COMEBACK seconds
//! after it is taken (a power weapon 30 to 60, drawn: POWER_BACK). A box of ammunition tops up the reserve of every gun
//! the hotdog holds to its CARRY, but not the power weapons, and a gun's pickup for a gun the hotdog already holds puts
//! its rounds in that gun's reserve, up to CARRY. Both are taken by walking over them, without a key (`Feast::scavenge`, PAD from the
//! pad's middle): a hotdog whose reserve has no room leaves them where they are. A gun the hotdog does not hold is still
//! taken with E (`Feast::pickup`, the pickup's number in a command): its rounds fill its magazine first and the rest
//! go to its reserve. Whoever takes a pickup first has it, as before: the commands as the server takes them, then the
//! hotdogs walking over pads by id, then the bots' errands.
//!
//! **The loadout** (`loadout`) is the same in every gun mode, for people and bots alike: the syrup rifle in hand and the
//! mustard pistol beside it, each with a full magazine and its SPARE rounds in reserve (the rifle 60 and 180, the pistol
//! 12 and 48). Nothing tops them up but pickups and boxes; coming back (Free For All) restores it. Free For All's
//! `ammo=inf`, and the sandbox's, make rounds never run out: magazines and reserves are kept full.
//!
//! **Online** a frame carries each gun's magazine as it always has (net.rs); the reserves and the reload under way go
//! to every game socket as `{type:'ammo',hotdogs:[Stock...]}` when they change (`Feast::stocks`), and a client sets its
//! copy of the round by them (`Feast::restock`). A reload is asked for by `net::button::RELOAD` in a command.
use crate::feast::{Feast, Player, Stage, FULL, GUNS, PISTOL, RIFLE, SNIPER};
use crate::map::AMMO_BOX;
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The rounds a gun's magazine holds, by weapon (feast.rs FULL).
pub const MAGAZINE: [i32; GUNS] = FULL;
/// The most rounds a hotdog carries in reserve for each gun: four magazines of mustard, three of ketchup and of syrup,
/// two of everything else.
pub const CARRY: [i32; GUNS] = [0, 48, 18, 180, 8, 80, 180, 300];
/// Seconds a reload takes, by weapon: the mustard pistol's squeeze bottle is quickest, the relish bazooka's glob slowest.
pub const RELOAD: [f32; GUNS] = [0., 1.1, 2.4, 2.0, 2.8, 1.3, 1.8, 1.5];
/// The spare rounds: what each gun a hotdog starts (or comes back) with has in reserve besides a full magazine. The
/// syrup rifle three magazines, the mustard pistol four; a gun the sandbox hands out its own.
pub const SPARE: [i32; GUNS] = [0, 48, 12, 90, 8, 40, 180, 150];
/// The guns every hotdog starts a round (and comes back) with in every gun mode, person and bot alike, the first in
/// hand: the syrup rifle and the mustard pistol (`loadout`).
pub const LOADOUT: [usize; 2] = [RIFLE, PISTOL];
/// The power weapons, the ketchup sniper and the relish bazooka: scarce in every gun mode. Their pickups give fewer
/// rounds (PICKUP), a box of ammunition does not top them up, and one taken comes back a random POWER_BACK[0] to
/// POWER_BACK[1] seconds later, drawn afresh from the game's seed each time it is taken (`Feast::comeback`). Out of
/// rounds, a power weapon is held empty, as any gun is.
pub const POWER: [usize; 2] = [SNIPER, 4];
pub const POWER_BACK: [f32; 2] = [30., 60.];
/// What a gun's pickup gives, by weapon, in every gun mode and whatever the map says: a magazine (FULL), but the power
/// weapons 4 rounds (the sniper) and 2 (the bazooka).
pub const PICKUP: [i32; GUNS] = [0, FULL[1], 4, FULL[3], 2, FULL[5], FULL[6], FULL[7]];
/// Seconds a pickup takes to come back once taken, in every gun mode and whatever the map says: a gun's, and a box of
/// ammunition's (the power weapons: POWER_BACK).
pub const COMEBACK: [f32; 2] = [20., 12.];
/// Whether the gun `w` is a power weapon (POWER).
pub fn power(w: usize) -> bool {
    POWER.contains(&w)
}
/// What a pickup of `weapon` gives and how long it takes to come back (a power weapon's is drawn as it is taken).
pub fn stock(weapon: usize) -> (i32, f32) {
    if weapon == AMMO_BOX { (0, COMEBACK[1]) } else { (PICKUP.get(weapon).copied().unwrap_or(0), COMEBACK[0]) }
}
/// Sets a hotdog's guns as every gun mode starts them (LOADOUT): each with a full magazine and its SPARE rounds in
/// reserve, the syrup rifle in hand; nothing else held and no reload under way.
pub fn loadout(p: &mut Player) {
    (p.owned, p.ammo, p.reserve, p.reload) = ([false; GUNS], [0; GUNS], [0; GUNS], None);
    p.ammo[0] = -1;
    for w in LOADOUT {
        p.owned[w] = true;
        (p.ammo[w], p.reserve[w]) = (MAGAZINE[w], SPARE[w].min(CARRY[w]));
    }
    (p.weapon, p.gun) = (LOADOUT[0], LOADOUT[0]);
}
/// How near a pad's middle a hotdog's feet (lifted 4 cm, as `pickup` measures) must be for it to walk over the pickup.
pub const PAD: f32 = 0.12;

/// A reload under way: the gun, the round's clock when it began, how long it takes, and the seconds it has left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reload {
    pub gun: usize,
    pub at: f32,
    pub of: f32,
    pub left: f32,
}
impl Reload {
    /// How far through it is, 0 to 1.
    pub fn progress(&self) -> f32 {
        if self.of <= 0. { 1. } else { (1. - self.left / self.of).clamp(0., 1.) }
    }
}
/// What a frame does not say of a hotdog's guns (`{type:'ammo'}`): its reserve by gun, and the reload under way (`gun`
/// 0 for none; `at` the round's clock it began, `of` how long it takes). The same while nothing changes, so it is only
/// sent when something does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stock {
    pub id: u32,
    pub reserve: [i32; GUNS],
    pub gun: usize,
    pub at: f32,
    pub of: f32,
}

/// A gun's rounds altogether: its magazine and its reserve.
pub fn rounds(p: &Player, w: usize) -> i32 {
    p.ammo.get(w).copied().unwrap_or(0).max(0) + p.reserve.get(w).copied().unwrap_or(0).max(0)
}
/// Puts whatever a gun holds beyond a magazine into its reserve (up to CARRY): a loadout written as rounds made into a
/// magazine and a reserve.
pub fn split(p: &mut Player) {
    for w in 1..GUNS {
        if p.ammo[w] > MAGAZINE[w] {
            p.reserve[w] = (p.reserve[w] + p.ammo[w] - MAGAZINE[w]).min(CARRY[w]);
            p.ammo[w] = MAGAZINE[w];
        }
    }
}
/// Whether a pickup gives a hotdog nothing but rounds: a box of ammunition, or the pickup of a gun it holds.
pub fn ammo_only(p: &Player, weapon: usize) -> bool {
    weapon == AMMO_BOX || (weapon < GUNS && p.owned[weapon])
}

impl Feast {
    /// R: starts reloading the gun in hand. Refused (false) where it cannot fire anyway (behind Forts' forcefield, after
    /// the round, at a card table, in Kung Fu), with its hands full, dead, already reloading, with a full magazine or an
    /// empty reserve. A syrup burst under way stops.
    pub fn reload(&mut self, id: u32) -> bool {
        if matches!(self.stage, Stage::RoundOver | Stage::Build) || self.cards.is_some() || self.kungfu.is_some() || self.laden(id) {
            return false;
        }
        let (now, tune) = (self.elapsed, self.tune.reload);
        let Some(p) = self.player_mut(id) else {
            return false;
        };
        let w = p.weapon;
        if !p.alive || !(1..GUNS).contains(&w) || !p.owned[w] || p.reload.is_some() || p.ammo[w] >= MAGAZINE[w] || p.reserve[w] <= 0 {
            return false;
        }
        let of = tune[w].max(0.);
        (p.reload, p.volley) = (Some(Reload { gun: w, at: now, of, left: of }), 0);
        true
    }
    /// A bot's reload: the gun in hand is reloaded when its magazine is below `share` of a full one (0 only when it is
    /// empty) and there is something to reload it from.
    pub(crate) fn top_up(&mut self, id: u32, share: f32) -> bool {
        let due = self.player(id).is_some_and(|p| {
            let w = p.weapon;
            (1..GUNS).contains(&w) && p.reload.is_none() && p.reserve[w] > 0 && (p.ammo[w] <= 0 || (p.ammo[w] as f32) < MAGAZINE[w] as f32 * share)
        });
        due && self.reload(id)
    }
    /// Whether a hotdog is reloading.
    pub fn reloading(&self, id: u32) -> bool {
        self.player(id).is_some_and(|p| p.reload.is_some())
    }
    /// One step of every reload under way: one whose hotdog has died, filled its hands or taken another gun in hand is
    /// cancelled; one whose time is up fills the magazine from the reserve.
    pub(crate) fn reloads(&mut self, dt: f32) {
        for i in 0..self.players.len() {
            if self.players[i].reload.is_none() {
                continue;
            }
            let laden = self.laden(self.players[i].id);
            let p = &mut self.players[i];
            let Some(mut r) = p.reload else { continue };
            if !p.alive || laden || p.weapon != r.gun || !p.owned.get(r.gun).copied().unwrap_or(false) {
                p.reload = None;
                continue;
            }
            r.left -= dt;
            if r.left > 1e-6 {
                p.reload = Some(r);
                continue;
            }
            let w = r.gun;
            let n = (MAGAZINE[w] - p.ammo[w]).clamp(0, p.reserve[w].max(0));
            (p.ammo[w], p.reserve[w], p.reload) = (p.ammo[w] + n, p.reserve[w] - n, None);
        }
    }
    /// Every living hotdog takes the pickups it walks over that give it rounds alone (`ammo_only`): a box of
    /// ammunition, or the pickup of a gun it holds (see the module's documentation). One its reserves have no room for
    /// is left where it is (`pickup` refuses it). By id, after the commands and before the bots.
    pub(crate) fn scavenge(&mut self) {
        if self.cards.is_some() || self.kungfu.is_some() || self.rules.unlimited() || self.stage == Stage::RoundOver {
            return;
        }
        for i in 0..self.players.len() {
            let p = &self.players[i];
            if !p.alive || p.consumed {
                continue;
            }
            let (id, feet) = (p.id, Vec3::from_array(p.p) + Vec3::Y * 0.04);
            let over: Vec<usize> = self
                .pickups
                .iter()
                .filter(|item| item.ready <= self.elapsed && Vec3::from_array(item.p).distance(feet) <= PAD && ammo_only(p, item.weapon))
                .map(|item| item.id)
                .collect();
            for item in over {
                self.pickup(id, item);
            }
        }
    }
    /// Every hotdog's reserve and reload, for the clients (`{type:'ammo'}`).
    pub fn stocks(&self) -> Vec<Stock> {
        self.players
            .iter()
            .map(|p| {
                let r = p.reload.unwrap_or_default();
                Stock { id: p.id, reserve: p.reserve, gun: r.gun, at: r.at, of: r.of }
            })
            .collect()
    }
    /// A client's copy of the round takes the server's word on the reserves and reloads (`stocks`).
    pub fn restock(&mut self, stocks: &[Stock]) {
        let now = self.elapsed;
        for s in stocks {
            if let Some(p) = self.player_mut(s.id) {
                p.reserve = s.reserve;
                p.reload = (s.gun > 0).then(|| Reload { gun: s.gun, at: s.at, of: s.of, left: (s.at + s.of - now).clamp(0., s.of.max(0.)) });
            }
        }
    }
    /// A hotdog's guns as every gun mode starts them: every gun it holds (the sandbox's loadout) with a full magazine
    /// and its SPARE rounds in reserve.
    pub(crate) fn spare(&mut self, id: u32) {
        if let Some(p) = self.player_mut(id) {
            for w in 1..GUNS {
                if p.owned[w] {
                    (p.ammo[w], p.reserve[w]) = (MAGAZINE[w], SPARE[w].min(CARRY[w]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feast::{FLOOR, MAYO, RIFLE};
    use crate::map::Map;
    use crate::rules::Rules;

    /// A bare counter with a ketchup sniper's pickup (the map says 6 rounds; the rules give 4) at (1, 0.5), a mustard
    /// pistol's (12) at (1.5, 0.5) and a box of ammunition at (2, 0.5); hotdog 1 (a person) at (0.3, 0.5) with the guns
    /// every gun mode starts it with (the rifle's 60 and 180, the pistol's 12 and 48); the others parked at the far end;
    /// nobody is a bot, and the warmup never ends.
    fn counter() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Counter", "bounds": [0, 0, 3, 1], "starts": [{"p": [0.3, 0.5]}], "spawns": [{"p": [2.9, 0.1]}],
            "pickups": [{"p": [1, 0.5], "weapon": 2, "ammo": 6}, {"p": [1.5, 0.5], "weapon": 1, "ammo": 12}, {"p": [2, 0.5], "weapon": 0}]}"#,
        )
        .unwrap();
        let mut s = Feast::with_map(map, false);
        for (i, p) in s.players.iter_mut().enumerate() {
            p.bot = false;
            if i > 0 {
                p.p = [2.85, FLOOR, 0.1 + 0.18 * i as f32];
            }
        }
        s.warmup = f32::INFINITY;
        s.embody();
        s
    }
    const SNIPER: usize = 0;
    const MUSTARD: usize = 1;
    const BOX: usize = 2;
    fn stand(s: &mut Feast, id: u32, x: f32, z: f32) {
        s.player_mut(id).unwrap().p = [x, FLOOR, z];
        s.embody();
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 60.).round() as usize {
            s.tick(1. / 60.);
        }
    }
    fn me(s: &Feast) -> &Player {
        s.player(1).unwrap()
    }
    fn there(s: &Feast, item: usize) -> bool {
        s.pickups[item].ready <= s.elapsed
    }

    #[test]
    fn walking_over_the_pickup_of_a_gun_held_takes_its_rounds_into_the_reserve_without_a_key_and_a_full_reserve_leaves_it() {
        let mut s = counter();
        assert_eq!((me(&s).ammo[1], me(&s).reserve[1]), (12, 48));
        // (Its pistol run down to 3, nothing in reserve.)
        (s.player_mut(1).unwrap().ammo[1], s.player_mut(1).unwrap().reserve[1]) = (3, 0);
        // Over the mustard pistol's pad: its 12 rounds go into the pistol's reserve; the magazine is as it was.
        stand(&mut s, 1, 1.5, 0.5);
        s.tick(1. / 60.);
        assert_eq!((me(&s).ammo[1], me(&s).reserve[1], me(&s).weapon), (3, 12, RIFLE));
        assert!(!there(&s, MUSTARD), "taken");
        // Standing there when it comes back with the reserve full: it is left where it is.
        s.player_mut(1).unwrap().reserve[1] = CARRY[1];
        run(&mut s, 20.1);
        assert!(there(&s, MUSTARD) && me(&s).reserve[1] == CARRY[1], "left on its pad");
        // Room for some of it: as many as there is room for, and the pickup is gone.
        s.player_mut(1).unwrap().reserve[1] = CARRY[1] - 5;
        s.tick(1. / 60.);
        assert_eq!(me(&s).reserve[1], CARRY[1]);
        assert!(!there(&s, MUSTARD));
        // A gun it does not hold is not taken by walking over it: E takes it, its rounds filling its magazine first.
        stand(&mut s, 1, 1., 0.5);
        run(&mut s, 0.5);
        assert!(!me(&s).owned[2] && there(&s, SNIPER), "the sniper waits for E");
        assert!(s.pickup(1, SNIPER));
        assert_eq!((me(&s).owned[2], me(&s).ammo[2], me(&s).reserve[2], me(&s).weapon), (true, 4, 0, 2), "a power weapon's four");
        // Held now: the next sniper pickup (back 30 to 60 s later), walked over, goes into its reserve.
        run(&mut s, POWER_BACK[1] + 0.1);
        assert_eq!((me(&s).ammo[2], me(&s).reserve[2]), (4, 4));
        // A box of ammunition walked over tops up every reserve but the power weapon's, and is left for a hotdog whose
        // reserves are full.
        s.player_mut(1).unwrap().reserve[1] = 0;
        stand(&mut s, 1, 2., 0.5);
        s.tick(1. / 60.);
        assert_eq!((me(&s).reserve[1], me(&s).reserve[2]), (CARRY[1], 4));
        assert!(!there(&s, BOX));
        run(&mut s, 12.1);
        assert!(there(&s, BOX), "full: the box stays");
    }

    #[test]
    fn whoever_reaches_a_pickup_first_has_it_a_press_of_e_before_a_hotdog_walking_over_it_and_hotdogs_by_id() {
        let mut s = counter();
        // Hotdogs 1 and 2 both hold the pistol and stand on its pad: the first by id takes its rounds.
        stand(&mut s, 2, 1.5, 0.5);
        stand(&mut s, 1, 1.5, 0.5);
        (s.player_mut(1).unwrap().reserve[1], s.player_mut(2).unwrap().reserve[1]) = (0, 0);
        s.tick(1. / 60.);
        assert_eq!((me(&s).reserve[1], s.player(2).unwrap().reserve[1]), (12, 0));
        // The sniper: hotdog 2 holds one and walks over its pad, hotdog 1 presses E there in the same step. The command
        // comes first (the server applies the commands, then steps the round): hotdog 1 has it.
        let p = s.player_mut(2).unwrap();
        (p.owned, p.ammo, p.reserve) = ([false, false, true, false, false, false, true, false], [-1, 0, 1, 0, 0, 0, 60, 0], [0; GUNS]);
        stand(&mut s, 2, 1., 0.5);
        stand(&mut s, 1, 1., 0.5);
        assert!(s.pickup(1, SNIPER));
        s.tick(1. / 60.);
        assert_eq!((me(&s).ammo[2], s.player(2).unwrap().reserve[2]), (4, 0));
    }

    #[test]
    fn r_reloads_the_gun_in_hand_in_its_reload_time_and_it_cannot_fire_meanwhile() {
        let mut s = counter();
        let d = Vec3::new(1., 0., 0.);
        (s.player_mut(1).unwrap().ammo[RIFLE], s.player_mut(1).unwrap().reserve[RIFLE]) = (10, 100);
        assert!(s.reload(1));
        assert!(!s.reload(1), "already reloading");
        assert!(!s.fire(1, d, RIFLE), "not while reloading");
        let steps = (RELOAD[RIFLE] * 60.).round() as usize;
        for _ in 0..steps - 1 {
            s.tick(1. / 60.);
        }
        assert!(s.reloading(1) && me(&s).ammo[RIFLE] == 10, "not yet");
        s.tick(1. / 60.);
        s.tick(1. / 60.);
        assert!(!s.reloading(1));
        assert_eq!((me(&s).ammo[RIFLE], me(&s).reserve[RIFLE]), (60, 50));
        assert!(s.fire(1, d, RIFLE));
        // A full magazine, or an empty reserve: nothing to do.
        let mut s = counter();
        assert!(!s.reload(1), "a full magazine");
        (s.player_mut(1).unwrap().ammo[RIFLE], s.player_mut(1).unwrap().reserve[RIFLE]) = (5, 0);
        assert!(!s.reload(1), "nothing in reserve");
        // The reserve short of a magazine: what there is.
        s.player_mut(1).unwrap().reserve[RIFLE] = 20;
        assert!(s.reload(1));
        run(&mut s, RELOAD[RIFLE] + 0.05);
        assert_eq!((me(&s).ammo[RIFLE], me(&s).reserve[RIFLE]), (25, 0));
        // Each gun its own time: the pistol's quickest, the bazooka's slowest.
        assert!((1..GUNS).all(|w| RELOAD[1] <= RELOAD[w] && RELOAD[w] <= RELOAD[4]));
        assert_eq!(s.tune.reload, RELOAD);
    }

    #[test]
    fn taking_another_gun_in_hand_a_strike_picking_up_or_dying_cancels_a_reload_and_nothing_goes_in() {
        let mut s = counter();
        (s.player_mut(1).unwrap().ammo[RIFLE], s.player_mut(1).unwrap().reserve[RIFLE]) = (10, 100);
        assert!(s.reload(1));
        run(&mut s, 1.);
        s.select(1, 1);
        assert!(!s.reloading(1));
        s.select(1, RIFLE);
        run(&mut s, RELOAD[RIFLE]);
        assert_eq!((me(&s).ammo[RIFLE], me(&s).reserve[RIFLE]), (10, 100), "the rifle was not reloaded");
        // The same gun again changes nothing; a strike cancels it.
        assert!(s.reload(1));
        s.select(1, RIFLE);
        assert!(s.reloading(1));
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(s.melee(1, Vec3::X));
        assert!(!s.reloading(1));
        // Its hands filled (a weapon of 0 is no gun) or its death cancel it too.
        run(&mut s, 0.5);
        assert!(s.reload(1));
        s.player_mut(1).unwrap().weapon = 0;
        s.tick(1. / 60.);
        assert!(!s.reloading(1));
        s.player_mut(1).unwrap().weapon = RIFLE;
        assert!(s.reload(1));
        s.hurt(1, 500.);
        s.tick(1. / 60.);
        assert!(!s.reloading(1));
    }

    #[test]
    fn the_trigger_on_an_empty_magazine_reloads_it_from_the_reserve_and_on_an_empty_gun_does_nothing() {
        let mut s = counter();
        let d = Vec3::new(1., 0., 0.);
        (s.player_mut(1).unwrap().ammo[RIFLE], s.player_mut(1).unwrap().reserve[RIFLE]) = (0, 5);
        assert!(!s.fire(1, d, RIFLE) && s.reloading(1), "it reloads instead");
        run(&mut s, RELOAD[RIFLE] + 0.05);
        assert_eq!((me(&s).ammo[RIFLE], me(&s).reserve[RIFLE]), (5, 0));
        // The pistol's magazine empty with rounds in reserve, from the rifle: it is taken in hand and reloaded.
        (s.player_mut(1).unwrap().ammo[1], s.player_mut(1).unwrap().reserve[1]) = (0, 7);
        assert!(!s.fire(1, d, 1) && s.reloading(1) && me(&s).weapon == 1);
        run(&mut s, RELOAD[1] + 0.05);
        assert_eq!((me(&s).ammo[1], me(&s).reserve[1]), (7, 0));
        // Nothing left at all: the trigger does nothing, and the gun can still strike.
        (s.player_mut(1).unwrap().ammo[1], s.player_mut(1).unwrap().cooldown) = (0, 0.);
        assert!(!s.fire(1, d, 1) && !s.reloading(1));
        assert!(s.melee(1, d));
        // A syrup burst stops when its reload begins.
        let mut s = counter();
        (s.player_mut(1).unwrap().ammo[RIFLE], s.player_mut(1).unwrap().reserve[RIFLE]) = (2, 60);
        assert!(s.fire(1, d, RIFLE));
        run(&mut s, 0.2);
        assert_eq!(me(&s).ammo[RIFLE], 0, "two rounds of the burst, and it ran dry");
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(!s.fire(1, d, RIFLE) && s.reloading(1));
    }

    #[test]
    fn free_for_all_rounds_run_out_its_guns_start_with_spare_magazines_and_coming_back_restores_them() {
        let mut s = counter();
        s.set_rules(Rules::parse("mode=ffa"));
        assert!(s.rules.finite_ammo && !s.rules.unlimited());
        for p in &s.players {
            assert_eq!((p.ammo[RIFLE], p.reserve[RIFLE], p.ammo[1], p.reserve[1]), (60, 180, 12, 48), "{}", p.name);
        }
        // Fired dry: twelve rounds a magazine, reloaded four times, and then nothing; nothing tops it up.
        let mut fired = 0;
        for _ in 0..6000 {
            let p = s.player_mut(1).unwrap();
            (p.weapon, p.gun, p.cooldown, p.shield) = (1, 1, 0., 0.);
            stand(&mut s, 1, 0.3, 0.5);
            if s.fire(1, Vec3::new(0., 0.5, -1.).normalize(), 1) {
                fired += 1;
            }
            s.tick(1. / 60.);
            if magazine_empty(&s) {
                break;
            }
        }
        assert_eq!(fired, 60);
        assert_eq!((me(&s).ammo[1], me(&s).reserve[1]), (0, 0));
        s.player_mut(1).unwrap().cooldown = 0.;
        assert!(!s.fire(1, Vec3::X, 1) && !s.reloading(1));
        run(&mut s, 1.);
        assert_eq!(rounds(me(&s), 1), 0, "still dry: nothing tops it up");
        // Dead and back: the loadout it started with.
        s.hurt(1, 500.);
        run(&mut s, crate::rules::FFA_RESPAWN + 0.1);
        assert!(me(&s).alive);
        assert_eq!((me(&s).ammo[RIFLE], me(&s).reserve[RIFLE], me(&s).ammo[1], me(&s).reserve[1]), (60, 180, 12, 48));
        // The rule is the rules' to say: Free For All with `ammo=inf` keeps every gun full, as it always did.
        let mut s = counter();
        s.set_rules(Rules::parse("mode=ffa ammo=inf"));
        assert!(!s.rules.finite_ammo && s.rules.unlimited());
        assert_eq!(s.rules.text(), "mode=ffa ammo=inf");
        assert_eq!(Rules::parse(&s.rules.text()), s.rules);
        s.player_mut(1).unwrap().ammo[1] = 0;
        s.tick(1. / 60.);
        assert_eq!((me(&s).ammo[1], me(&s).reserve[1]), (12, CARRY[1]));
        // The siege starts with the same loadout, and is not topped up either.
        let mut s = counter();
        s.player_mut(1).unwrap().ammo[1] = 0;
        s.tick(1. / 60.);
        assert_eq!((me(&s).ammo[1], me(&s).reserve[1], me(&s).ammo[RIFLE], me(&s).reserve[RIFLE]), (0, 48, 60, 180));
    }
    fn magazine_empty(s: &Feast) -> bool {
        rounds(me(s), 1) == 0
    }

    #[test]
    fn a_bot_reloads_an_empty_magazine_and_run_dry_it_takes_its_other_gun_fetches_more_or_bites() {
        let mut s = counter();
        s.set_rules(Rules::parse("mode=ffa"));
        // Hotdog 2, a bot, faces hotdog 1 (a person, standing still) 0.8 m off, its rifle's magazine empty.
        let face = |s: &mut Feast| {
            for (i, p) in s.players.iter_mut().enumerate() {
                if i > 1 {
                    (p.alive, p.consumed, p.hp) = (false, true, 0.);
                }
            }
            stand(s, 1, 0.3, 0.5);
            stand(s, 2, 1.1, 0.5);
            let p = s.player_mut(2).unwrap();
            (p.bot, p.weapon, p.gun, p.shield) = (true, RIFLE, RIFLE, 0.);
            s.player_mut(1).unwrap().hp = 1e6;
            s.famine_at = s.elapsed - 10.;
        };
        face(&mut s);
        s.player_mut(2).unwrap().ammo[RIFLE] = 0;
        s.tick(1. / 60.);
        assert!(s.reloading(2), "it reloads at once");
        run(&mut s, RELOAD[RIFLE] + 0.6);
        let bot = s.player(2).unwrap();
        assert!(bot.reserve[RIFLE] < 180, "{:?}", bot.reserve);
        assert!(s.shots.iter().any(|shot| shot.owner == 2) || bot.ammo[RIFLE] > 0, "and goes on shooting");
        // Its rifle dry altogether: it shoots the pistol.
        let mut s = counter();
        s.set_rules(Rules::parse("mode=ffa"));
        face(&mut s);
        (s.player_mut(2).unwrap().ammo[RIFLE], s.player_mut(2).unwrap().reserve[RIFLE]) = (0, 0);
        let first = s.shots.iter().map(|shot| shot.id + 1).max().unwrap_or(0);
        // (Watched all the while: a shot is on the list for 0.4 s, and a bot takes its time between shots, aim.rs.)
        let mut pistol = false;
        for _ in 0..60 {
            run(&mut s, 1. / 60.);
            pistol |= s.shots.iter().any(|shot| shot.id >= first && shot.owner == 2 && shot.weapon == 1);
        }
        assert!(pistol, "the pistol");
        // Everything dry, with the pistol's pickup on the counter: it fetches it.
        let mut s = counter();
        s.set_rules(Rules::parse("mode=ffa"));
        face(&mut s);
        let p = s.player_mut(2).unwrap();
        (p.ammo, p.reserve) = ([-1, 0, 0, 0, 0, 0, 0, 0], [0; GUNS]);
        run(&mut s, 4.);
        let bot = s.player(2).unwrap();
        assert!((1..GUNS).any(|w| rounds(bot, w) > 0), "it fetched a gun or rounds: {:?} {:?}", bot.ammo, bot.reserve);
        // Everything dry and nothing to be had: it closes in and bites.
        let mut s = counter();
        s.set_rules(Rules::parse("mode=ffa"));
        for item in &mut s.pickups {
            item.ready = f32::INFINITY;
        }
        face(&mut s);
        let p = s.player_mut(2).unwrap();
        (p.ammo, p.reserve) = ([-1, 0, 0, 0, 0, 0, 0, 0], [0; GUNS]);
        run(&mut s, 4.);
        assert!(s.player(2).unwrap().swings > 0 && s.player(1).unwrap().hp < 1e6, "it bit");
    }

    #[test]
    fn the_reserves_and_the_reload_reach_a_client_as_the_server_has_them() {
        let mut s = counter();
        (s.player_mut(1).unwrap().ammo[RIFLE], s.player_mut(1).unwrap().reserve[RIFLE]) = (10, 100);
        s.player_mut(3).unwrap().reserve[MAYO] = 7;
        run(&mut s, 0.5);
        assert!(s.reload(1));
        run(&mut s, 0.5);
        let stocks = s.stocks();
        assert_eq!(stocks.len(), s.players.len());
        let mine = &stocks[0];
        assert_eq!((mine.id, mine.reserve[RIFLE], mine.gun, mine.of), (1, 100, RIFLE, RELOAD[RIFLE]));
        assert_eq!(stocks[2].reserve[MAYO], 7);
        // The same while nothing changes: it need only be sent when something does.
        s.tick(1. / 60.);
        assert_eq!(s.stocks(), stocks);
        // Through JSON and into a client's copy of the round, at the same moment: what the server has.
        let told: Vec<Stock> = serde_json::from_str(&serde_json::to_string(&stocks).unwrap()).unwrap();
        let mut copy = counter();
        copy.elapsed = s.elapsed;
        copy.restock(&told);
        let (a, b) = (copy.player(1).unwrap(), s.player(1).unwrap());
        assert_eq!(a.reserve, b.reserve);
        let (ra, rb) = (a.reload.unwrap(), b.reload.unwrap());
        assert!(ra.gun == rb.gun && (ra.left - rb.left).abs() < 1e-3 && (ra.progress() - rb.progress()).abs() < 1e-3, "{ra:?} {rb:?}");
        assert_eq!(copy.player(3).unwrap().reserve[MAYO], 7);
    }
}

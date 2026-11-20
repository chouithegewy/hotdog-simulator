// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The numbers the weapons and the hotdogs go by, as values a round can change while it plays (`Feast::tune`): every
//! read of a gun's speed, fall, size, reach, time between shots, damage and splash goes through here, so that the
//! sandbox's tuning panel (and the server's --tune) changes the game live, for the bots too. `Tuning::default()` is
//! the game as its constants have it (feast.rs, actor.rs).
//!
//! As text (what the panel copies, what --tune and the `tune` message take, what the server's `tuning` message says):
//! `key=value` words apart by spaces or commas, a key being `<group>.<field>`: a gun's group is its name (mustard,
//! sniper, hotsauce, relish, bugspray, syrup, mayo, melee for the strike; pistol, ketchup, flame, bazooka, spray,
//! rifle also do), with speed (m/s), fall (m/s², its arc), size (m), range (m), delay (s between shots; mayo's flow is
//! droplets a second instead), hotdog and zombie (damage), splash (m), spray (the can's damage to a swarm), recoil (how
//! much the view kicks and the aim blooms, 1 as it is), reload (s); the syrup rifle adds burst, gap and climb. `stim.` walk, jump, damage, time,
//! wait, squirt and flow; `move.` walk and jump. `Tuning::text` writes only what differs from the default.
use crate::actor;
use crate::feast::{CLIMB, DELAY, DROOP, GUNS, MAYO, PLAYER_DAMAGE, RANGE, RIFLE, ROUND_RADIUS, ROUND_SPEED, SPLASH, SPRAY_DAMAGE, STIM, STIM_FLOW, STIM_SQUIRT, STIM_WAIT, VOLLEY, VOLLEY_GAP, ZOMBIE_DAMAGE};
use crate::rules::{WEAPONS, weapon};
use serde::Serialize;

/// See the module's documentation.
#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    pub speed: [f32; GUNS],
    pub fall: [f32; GUNS],
    pub size: [f32; GUNS],
    pub range: [f32; GUNS],
    pub delay: [f32; GUNS],
    pub splash: [f32; GUNS],
    pub zombie: [f32; GUNS],
    pub hotdog: [f32; GUNS],
    pub spray: [f32; GUNS],
    /// How hard each gun kicks the view, as a share of its own recoil (the client's), and the aim (recoil.rs).
    pub recoil: [f32; GUNS],
    /// Seconds each gun takes to reload (magazine.rs RELOAD).
    pub reload: [f32; GUNS],
    /// The syrup rifle's burst: rounds, seconds between them, and the climb of each.
    pub burst: u32,
    pub gap: f32,
    pub climb: f32,
    /// Stim: the walk's and the jump's (m/s) while it lasts, the damage taken from rivals (times), how long it lasts
    /// and how long until the next, and the mayonnaise's squirt and flow (times).
    pub stim_walk: f32,
    pub stim_jump: f32,
    pub stim_damage: f32,
    pub stim_time: f32,
    pub stim_wait: f32,
    pub stim_squirt: f32,
    pub stim_flow: f32,
    /// A person's walk and jump, m/s.
    pub walk: f32,
    pub jump: f32,
}
impl Default for Tuning {
    fn default() -> Tuning {
        Tuning {
            speed: ROUND_SPEED,
            fall: DROOP,
            size: ROUND_RADIUS,
            range: RANGE,
            delay: DELAY,
            splash: SPLASH,
            zombie: ZOMBIE_DAMAGE,
            hotdog: PLAYER_DAMAGE,
            spray: SPRAY_DAMAGE,
            recoil: [1.; GUNS],
            reload: crate::magazine::RELOAD,
            burst: VOLLEY,
            gap: VOLLEY_GAP,
            climb: CLIMB,
            stim_walk: actor::STIM_PACE as f32,
            stim_jump: actor::STIM_JUMP as f32,
            stim_damage: 2.,
            stim_time: STIM,
            stim_wait: STIM_WAIT,
            stim_squirt: STIM_SQUIRT,
            stim_flow: STIM_FLOW,
            walk: actor::WALK as f32,
            jump: actor::JUMP as f32,
        }
    }
}
/// One value the panel shows: its key, group and field, what it is called, its unit, its range and step, and its
/// default.
#[derive(Clone, Debug, Serialize)]
pub struct Knob {
    pub key: String,
    pub group: String,
    pub field: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
}
/// A value for the page: as short as it is written (0.35, not the 0.3499999940395355 a widened f32 gives).
pub fn clean(v: f32) -> f64 {
    format!("{v}").parse().unwrap_or(v as f64)
}
/// The groups as the panel lists them: id and title.
pub const GROUPS: [(&str, &str); 10] = [
    ("melee", "Melee strike"),
    ("mustard", "Mustard pistol"),
    ("sniper", "Ketchup sniper"),
    ("hotsauce", "Hot sauce thrower"),
    ("relish", "Relish bazooka"),
    ("bugspray", "Bug spray"),
    ("syrup", "Syrup rifle"),
    ("mayo", "Mayo squirt bottle"),
    ("stim", "Stim"),
    ("move", "Movement"),
];
/// A gun's fields: name, label, unit, min, max, step.
const GUN: [(&str, &str, &str, f32, f32, f32); 11] = [
    ("speed", "Shot speed", "m/s", 0.05, 100., 0.05),
    ("fall", "Gravity (arc)", "m/s²", 0., 200., 0.1),
    ("size", "Round size", "m", 0.002, 0.15, 0.001),
    ("range", "Range", "m", 0.05, 20., 0.05),
    ("delay", "Time between shots", "s", 0.02, 5., 0.01),
    ("hotdog", "Damage to hotdogs", "HP", 0., 300., 0.5),
    ("zombie", "Damage to zombies", "HP", 0., 300., 0.5),
    ("splash", "Splash radius", "m", 0., 1.5, 0.01),
    ("spray", "Damage to pests", "HP", 0., 200., 0.5),
    ("recoil", "Recoil", "×", 0., 4., 0.05),
    ("reload", "Reload time", "s", 0., 10., 0.05),
];
const RIFLE_FIELDS: [(&str, &str, &str, f32, f32, f32); 3] = [("burst", "Rounds a burst", "", 1., 12., 1.), ("gap", "Time between burst rounds", "s", 0.01, 0.5, 0.005), ("climb", "Climb a burst round", "", 0., 0.1, 0.001)];
const FLOW: (&str, &str, &str, f32, f32, f32) = ("flow", "Flow", "drops/s", 1., 60., 0.5);
const STIMS: [(&str, &str, &str, f32, f32, f32); 7] = [
    ("walk", "Walk ×", "×", 0.5, 4., 0.05),
    ("jump", "Jump speed", "m/s", 0.2, 4., 0.01),
    ("damage", "Damage taken ×", "×", 0., 5., 0.05),
    ("time", "Lasts", "s", 0.5, 120., 0.5),
    ("wait", "Cooldown", "s", 0., 120., 0.5),
    ("squirt", "Mayo squirt ×", "×", 0.2, 5., 0.05),
    ("flow", "Mayo flow ×", "×", 0.2, 5., 0.05),
];
const MOVES: [(&str, &str, &str, f32, f32, f32); 2] = [("walk", "Walk speed", "m/s", 0.1, 3., 0.01), ("jump", "Jump speed", "m/s", 0.2, 3., 0.01)];

/// The group a word names: a gun (by any of its names), stim or move.
fn group(word: &str) -> Option<String> {
    match word {
        "stim" | "move" | "melee" => Some(word.into()),
        _ => weapon(word).map(|w| WEAPONS[w].to_string()),
    }
}
impl Tuning {
    /// Every value there is, in the panel's order.
    pub fn knobs() -> Vec<Knob> {
        let base = Tuning::default();
        let mut out = vec![];
        let mut add = |group: &str, (field, label, unit, min, max, step): (&'static str, &'static str, &'static str, f32, f32, f32)| {
            let key = format!("{group}.{field}");
            let default = base.get(&key).unwrap_or(0.);
            out.push(Knob { key, group: group.into(), field, label, unit, min: clean(min), max: clean(max.max(default)), step: clean(step), default: clean(default) });
        };
        for (w, name) in WEAPONS.iter().enumerate() {
            for f in GUN {
                // The strike has no round; only the can sprays a swarm; the mayonnaise flows rather than fires.
                let round = !matches!(f.0, "speed" | "fall" | "size" | "splash" | "spray" | "recoil" | "reload");
                if (w == 0 && !round) || (f.0 == "spray" && w != 5) || (w == MAYO && f.0 == "delay") {
                    continue;
                }
                add(name, f);
                if w == MAYO && f.0 == "range" {
                    add(name, FLOW);
                }
            }
            if w == RIFLE {
                for f in RIFLE_FIELDS {
                    add(name, f);
                }
            }
        }
        for f in STIMS {
            add("stim", f);
        }
        for f in MOVES {
            add("move", f);
        }
        out
    }
    /// The value of a key (`<group>.<field>`, any of a gun's names), or None for no such value.
    pub fn get(&self, key: &str) -> Option<f32> {
        let (g, field) = key.trim().to_ascii_lowercase().split_once('.').map(|(a, b)| (a.to_string(), b.to_string()))?;
        let g = group(&g)?;
        Some(match (g.as_str(), field.as_str()) {
            ("stim", "walk") => self.stim_walk,
            ("stim", "jump") => self.stim_jump,
            ("stim", "damage") => self.stim_damage,
            ("stim", "time") => self.stim_time,
            ("stim", "wait") => self.stim_wait,
            ("stim", "squirt") => self.stim_squirt,
            ("stim", "flow") => self.stim_flow,
            ("move", "walk") => self.walk,
            ("move", "jump") => self.jump,
            ("syrup", "burst") => self.burst as f32,
            ("syrup", "gap") => self.gap,
            ("syrup", "climb") => self.climb,
            (_, "flow") => 1. / self.delay[weapon_of(&g)?].max(1e-3),
            (_, field) => self.table(field)?[weapon_of(&g)?],
        })
    }
    fn table(&self, field: &str) -> Option<&[f32; GUNS]> {
        Some(match field {
            "speed" => &self.speed,
            "fall" | "gravity" | "arc" => &self.fall,
            "size" | "radius" => &self.size,
            "range" | "reach" => &self.range,
            "delay" => &self.delay,
            "splash" => &self.splash,
            "zombie" => &self.zombie,
            "hotdog" | "damage" => &self.hotdog,
            "spray" => &self.spray,
            "recoil" => &self.recoil,
            "reload" => &self.reload,
            _ => return None,
        })
    }
    /// Sets a key to `value` (kept within its knob's range, so that nothing breaks). Returns whether there was such
    /// a key.
    pub fn set(&mut self, key: &str, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some((g, field)) = key.trim().to_ascii_lowercase().split_once('.').map(|(a, b)| (a.to_string(), b.to_string())) else {
            return false;
        };
        let Some(g) = group(&g) else {
            return false;
        };
        let field = match field.as_str() {
            "gravity" | "arc" => "fall".to_string(),
            "radius" => "size".into(),
            "reach" => "range".into(),
            "damage" if g != "stim" => "hotdog".into(),
            _ => field,
        };
        let key = format!("{g}.{field}");
        let Some(knob) = Tuning::knobs().into_iter().find(|k| k.key == key) else {
            return false;
        };
        let v = value.clamp(knob.min as f32, knob.max as f32);
        match (g.as_str(), field.as_str()) {
            ("stim", "walk") => self.stim_walk = v,
            ("stim", "jump") => self.stim_jump = v,
            ("stim", "damage") => self.stim_damage = v,
            ("stim", "time") => self.stim_time = v,
            ("stim", "wait") => self.stim_wait = v,
            ("stim", "squirt") => self.stim_squirt = v,
            ("stim", "flow") => self.stim_flow = v,
            ("move", "walk") => self.walk = v,
            ("move", "jump") => self.jump = v,
            ("syrup", "burst") => self.burst = v.round() as u32,
            ("syrup", "gap") => self.gap = v,
            ("syrup", "climb") => self.climb = v,
            (_, "flow") => self.delay[MAYO] = 1. / v,
            (_, field) => {
                let w = weapon_of(&g).unwrap_or(0);
                let table = match field {
                    "speed" => &mut self.speed,
                    "fall" => &mut self.fall,
                    "size" => &mut self.size,
                    "range" => &mut self.range,
                    "delay" => &mut self.delay,
                    "splash" => &mut self.splash,
                    "zombie" => &mut self.zombie,
                    "hotdog" => &mut self.hotdog,
                    "spray" => &mut self.spray,
                    "recoil" => &mut self.recoil,
                    "reload" => &mut self.reload,
                    _ => return false,
                };
                table[w] = v;
            }
        }
        true
    }
    /// Applies a line of `key=value` words (see the module's documentation); `reset` puts everything back, `reset
    /// <group>` one group. Returns the keys it did not know.
    pub fn apply(&mut self, text: &str) -> Vec<String> {
        let mut unknown = vec![];
        let words: Vec<&str> = text.split([' ', ',', ';', '\n', '\t', '&']).filter(|w| !w.is_empty()).collect();
        let mut i = 0;
        while i < words.len() {
            let word = words[i];
            i += 1;
            if word == "reset" {
                match words.get(i).filter(|w| !w.contains('=')).and_then(|w| group(w)) {
                    Some(g) => {
                        i += 1;
                        let base = Tuning::default();
                        for knob in Tuning::knobs().into_iter().filter(|k| k.group == g) {
                            self.set(&knob.key, base.get(&knob.key).unwrap_or(knob.default as f32));
                        }
                    }
                    None => *self = Tuning::default(),
                }
                continue;
            }
            match word.split_once('=').and_then(|(k, v)| Some((k, v.parse::<f32>().ok()?))) {
                Some((key, value)) if self.set(key, value) => {}
                _ => unknown.push(word.to_string()),
            }
        }
        unknown
    }
    /// The tuning `text` says, from the default.
    pub fn parse(text: &str) -> Tuning {
        let mut tuning = Tuning::default();
        tuning.apply(text);
        tuning
    }
    /// What differs from the default, as `key=value` words (empty for none).
    pub fn text(&self) -> String {
        let base = Tuning::default();
        Tuning::knobs()
            .into_iter()
            .filter_map(|k| {
                let (now, was) = (self.get(&k.key)?, base.get(&k.key)?);
                // (Mayo's delay shows as its flow.)
                ((now - was).abs() > 1e-6 * was.abs().max(1.)).then(|| format!("{}={}", k.key, round(now)))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    /// Every value by key, for the panel.
    pub fn values(&self) -> std::collections::BTreeMap<String, f64> {
        Tuning::knobs().into_iter().filter_map(|k| Some((k.key.clone(), clean(self.get(&k.key)?)))).collect()
    }
    /// A person's walking pace in m/s (net.rs `pace` with these numbers): the walk, slower crouching and prone,
    /// faster on stim, slower carrying something heavy.
    pub fn pace(&self, prone: bool, crouch: bool, stim: bool, burden: f32) -> f32 {
        let stance = if prone { actor::PRONE_PACE } else if crouch { actor::CROUCH_PACE } else { 1. } as f32;
        self.walk * stance * if stim { self.stim_walk } else { 1. } * burden
    }
    /// A person's jump, m/s: stimmed or not.
    pub fn leap(&self, stim: bool) -> f32 {
        if stim { self.stim_jump } else { self.jump }
    }
}
/// A gun's number from its group's name.
fn weapon_of(group: &str) -> Option<usize> {
    WEAPONS.iter().position(|w| *w == group)
}
/// A value as text: no more digits than it needs.
fn round(v: f32) -> String {
    let r = (v as f64 * 1e5).round() / 1e5;
    format!("{}", r as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_default_is_the_games_constants_and_says_nothing_has_changed() {
        let t = Tuning::default();
        assert_eq!((t.speed, t.delay, t.range), (ROUND_SPEED, DELAY, RANGE));
        assert_eq!(t.text(), "");
        assert_eq!(t.get("mustard.speed"), Some(ROUND_SPEED[1]));
        assert_eq!(t.get("ketchup.speed"), t.get("sniper.speed"));
        assert!((t.get("mayo.flow").unwrap() - 1. / DELAY[MAYO]).abs() < 1e-3);
        assert_eq!(t.pace(false, false, false, 1.), crate::net::pace(false, false, false, 1.));
        assert_eq!(t.pace(true, false, true, 0.8), crate::net::pace(true, false, true, 0.8));
        // Every knob reads back its default, and keys are unique.
        let knobs = Tuning::knobs();
        for k in &knobs {
            assert_eq!(t.get(&k.key).map(clean), Some(k.default), "{}", k.key);
            assert!(k.min <= k.default && k.default <= k.max, "{}", k.key);
        }
        assert_eq!(knobs.iter().find(|k| k.key == "mustard.delay").unwrap().default, 0.35, "as short as it is written");
        let mut keys: Vec<&str> = knobs.iter().map(|k| k.key.as_str()).collect();
        keys.dedup();
        assert_eq!(keys.len(), knobs.len());
        assert!(knobs.iter().any(|k| k.key == "mayo.flow") && !knobs.iter().any(|k| k.key == "mayo.delay"));
        assert!(knobs.iter().any(|k| k.key == "syrup.burst") && !knobs.iter().any(|k| k.key == "mustard.burst"));
    }
    #[test]
    fn a_round_leaves_at_the_tuned_speed_ends_at_the_tuned_reach_and_a_new_round_keeps_the_tuning() {
        use glam::Vec3;
        let mut s = crate::feast::Feast::new(false);
        s.tune.apply("mustard.speed=3 mustard.range=0.3 mustard.delay=1");
        assert!(s.fire(1, Vec3::new(0., 0., -1.), 1));
        assert!((Vec3::from_array(s.projectiles[0].v).length() - 3.).abs() < 1e-4);
        assert_eq!(s.player(1).unwrap().cooldown, 1.);
        for _ in 0..12 {
            s.flight(1. / 60.);
        }
        assert!(s.projectiles.is_empty(), "gone by 0.3 m");
        s.reset();
        assert_eq!(s.tune.speed[1], 3.);
    }
    #[test]
    fn text_sets_values_by_any_of_a_guns_names_and_writes_back_only_what_changed() {
        let mut t = Tuning::parse("mustard.speed=1.1,sniper.speed=12 flame.delay=0.2 bugspray.spray=40 syrup.burst=5 mayo.flow=20 stim.walk=2 move.jump=1.2");
        assert_eq!((t.speed[1], t.speed[2], t.delay[3], t.spray[5], t.burst, t.stim_walk, t.jump), (1.1, 12., 0.2, 40., 5, 2., 1.2));
        assert!((t.delay[MAYO] - 0.05).abs() < 1e-6);
        assert_eq!(t.text(), "mustard.speed=1.1 sniper.speed=12 hotsauce.delay=0.2 bugspray.spray=40 syrup.burst=5 mayo.flow=20 stim.walk=2 move.jump=1.2");
        assert_eq!(Tuning::parse(&t.text()), t);
        // Out of range is kept in range; nonsense is refused and named.
        assert_eq!(t.apply("relish.splash=99 mustard.speed=-3 nonsense.speed=1 mustard.colour=2 mustard.speed=x"), vec!["nonsense.speed=1", "mustard.colour=2", "mustard.speed=x"]);
        assert_eq!((t.splash[4], t.speed[1]), (1.5, 0.05));
        // Reset one group, then everything.
        t.apply("reset sniper");
        assert_eq!(t.speed[2], ROUND_SPEED[2]);
        assert_eq!(t.speed[1], 0.05);
        t.apply("reset");
        assert_eq!(t, Tuning::default());
    }
}

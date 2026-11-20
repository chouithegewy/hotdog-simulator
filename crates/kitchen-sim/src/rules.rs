// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What a game is played by beyond its map: its mode, and the dev sandbox. `Feast::rules` holds them (a new round keeps
//! them); the server sets them for a game, single player from the page's address, and a frame carries them (net.rs, as
//! `Rules::text`), so every client knows what it is playing.
//!
//! **The mode is the game's, not the map's** (`Mode`): `mode=siege`, `mode=forts`, `mode=ffa`, `mode=cards` or
//! `mode=kungfu`. Rules that name none play the mode the map was made for (its `mode` tag: `Mode::made_for`), so an old
//! link, `--map FILE` and a map's own listing play as they always did. Free For All plays on every gun map; the siege on
//! every map with a cat spawn point (all of them but the dojo, which is Kung Fu's); Forts on a map with the Forts
//! layout, two bases, a field and the bacon's spot (`Map::refuses` says why a map cannot have a mode). A map's Forts
//! zones are layout that any map may carry: played as anything but Forts they are left out.
//!
//! **The basics are the same in every gun mode** (the siege's wave and famine, Forts' build and fight, Free For All):
//! every hotdog, person or bot, starts (and comes back) with the syrup rifle in hand and the mustard pistol beside it,
//! each with a full magazine and its magazine.rs SPARE rounds in reserve (`magazine::loadout`), and rounds run out; a
//! gun's pickup gives what magazine.rs PICKUP says (one magazine, the power weapons fewer), whatever the map says, and
//! comes back after magazine.rs COMEBACK seconds (the power weapons after 30 to 60, drawn); a box of ammunition tops up
//! every gun but the power weapons; F is the strike along the aim (a kill from behind, half of MAX_HP from the front)
//! at whoever the mode lets a hotdog hurt; and killing a hotdog scores a kill and 100 points. What is left to each mode
//! is its objective: the wave's cats and no friendly fire, the famine's hunger, its pests and its healing bite (E eats
//! the dead), Forts' teams, build phase and bacon, and Free For All's respawns and spawn protection.
//!
//! **Free For All** is a mode of its own, on any map (a Forts map's zones and forcefield are left out: the hotdogs
//! use its floor and its pickups): everyone fights everyone, humans and bots, from the first second; there is no
//! zombie wave, no hunger, no pests and no bite. A round opens with the hotdogs spread over the map as far apart as it
//! allows, from a spot drawn at random (spawns.rs). A hotdog that dies is back FFA_RESPAWN seconds later, whole, at
//! a safe spot: none of the living within reach of it or able to see it, away from where it died and from where it
//! last came back, drawn from the best few (with nowhere safe, the spot furthest from the living); with SHIELD
//! seconds of spawn protection that its first shot or blow ends. Coming back restores the loadout. (`ammo=inf` with
//! `mode=ffa` makes rounds unlimited, as Free For All's once were: every magazine and reserve kept full; it sets
//! `Rules::finite_ammo` false.) The first to FFA_KILLS kills wins, or whoever has the most when FFA_TIME seconds are up
//! (the same number of kills at the top: a draw); then the score is shown, and the next round follows as a siege's does.
//!
//! **Fully Loaded** (`mode=cards`, cards.rs) is a mode of its own too, on any map: the five hotdogs stand round a little
//! card table on the microwave's top (`feast::table_seats`) and play it from the first second. No cats, no hunger, no
//! guns and no hit points (nothing the hotdogs do but flip or stop moves them); the table runs on `Feast::step` and its
//! game ends the round (the winner's alert, then the next round after eight seconds as a siege's does).
//!
//! **Kung Fu** (`mode=kungfu`, kungfu.rs) is Free For All with hands and feet only: no guns, pickups, carrying, eating
//! or stim; six fighting styles; respawns after kungfu::RESPAWN seconds at a start away from the living; the first to
//! kungfu::KOS KOs wins, or the most in kungfu::TIME seconds. A map whose mode is `kungfu` (the dojo) is always played
//! so, whatever the rules say; `mode=kungfu` plays it on any other map too.
//!
//! **The sandbox** is for trying things out (the page's `?sandbox`; online only on a server started with --dev): no
//! lobby, no countdown, no clock, no phase changes and no end. A siege map plays its free for all without hunger or
//! pests; a Forts map its fight, with the forcefield down (`field=on` keeps it up), no bacon and no stalemate cats.
//! A hotdog that dies is back SANDBOX_RESPAWN seconds later, at its start or (`respawn=here`) where it fell. What it
//! takes (all of it optional, as the page's address or a join's `sandbox` names it):
//!   weapons=mustard,sniper   the guns a person's hotdog starts and respawns with (names or numbers, or `all`: every
//!                            gun, Y cycles them); mustard|pistol 1, ketchup|sniper 2, hotsauce|flame 3,
//!                            relish|bazooka 4, bugspray|spray 5, syrup|rifle 6, mayo 7
//!   ammo=inf                 rounds never run out (for everyone)
//!   god                      a person's hotdog takes no damage (the hit is still on its record)
//!   bots=N                   how many bots play (0 to 5; the rest of the seats stand empty), or bots=idle|walk|fight
//!   botmode=idle|walk|fight  what they do: stand still, walk to and fro at their start (targets), or play (fight)
//!   zombies=on               cats come out of the microwave, a wave whenever the last is gone (off unless asked)
//!   field=on                 Forts: the forcefield stays up
//!   stim=inf                 a stim lasts until the hotdog dies, and can always be taken
//!   respawn=here             back where it fell rather than at its start
//!   mode=ffa                 the sandbox's rules on top of Free For All's (no end, no clock)
//!   mode=cards               Fully Loaded: the sandbox's other keys mean nothing at a card table
//!   mode=siege|forts         the map played as a siege, or as Forts where it has the layout
//! `map=<name>` chooses the map; it is the server's (and the page's) to read, not these rules'.
//!
//! **The bots' skill** (aim.rs), in every mode and with or without the sandbox: `skill=easy|normal|hard` (or
//! `bots=easy|normal|hard`), normal unless asked; `skill=perfect` is the machine's aim the bots once had, for tests and
//! trying things out. It is written in the rules' line only when it is not normal.
use crate::feast::{Feast, Damage, Stage, FALL, FULL, GUNS, NAMES};
use crate::map::Map;
use glam::Vec3;

/// The modes a game is played in: the three with guns (the siege, Forts, Free For All), and Fully Loaded and Kung Fu.
/// See the module's documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Siege,
    Forts,
    Ffa,
    Cards,
    KungFu,
    /// Titan (titan.rs): one giant hotdog against the rest.
    Titan,
}
impl Mode {
    pub const ALL: [Mode; 6] = [Mode::Siege, Mode::Forts, Mode::Ffa, Mode::Cards, Mode::KungFu, Mode::Titan];
    /// The gun modes, whose basics are the same (Titan's small hotdogs play by them).
    pub const GUNS: [Mode; 4] = [Mode::Siege, Mode::Forts, Mode::Ffa, Mode::Titan];
    /// Its name in a rules line, a page's address, the lobby and the server's --mode.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Siege => "siege",
            Mode::Forts => "forts",
            Mode::Ffa => "ffa",
            Mode::Cards => "cards",
            Mode::KungFu => crate::kungfu::MODE,
            Mode::Titan => crate::titan::MODE,
        }
    }
    /// What the screen and the lobby call it.
    pub fn title(self) -> &'static str {
        match self {
            Mode::Siege => "Siege",
            Mode::Forts => "Forts",
            Mode::Ffa => "Free For All",
            Mode::Cards => "Fully Loaded",
            Mode::KungFu => "Kung Fu",
            Mode::Titan => "Titan",
        }
    }
    pub fn named(name: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.name() == name)
    }
    /// Whether it is played with guns.
    pub fn guns(self) -> bool {
        Mode::GUNS.contains(&self)
    }
    /// The mode a map was made for: its `mode` tag (siege unless it says forts or kungfu). Rules that name no mode
    /// play it.
    pub fn made_for(map: &Map) -> Mode {
        Mode::named(&map.mode).unwrap_or(Mode::Siege)
    }
}
impl Map {
    /// Why a game of `mode` cannot be played on this map (None: it can). A Kung Fu map (the dojo) is Kung Fu's alone,
    /// and Kung Fu has no other; the siege needs a cat spawn point (every map has one) and Forts the Forts layout (two
    /// bases, a field and the bacon's spot, or a map made for Forts, whose missing zones are filled in:
    /// `Map::fortress`). Fully Loaded's table stands on any map's microwave or start (the server keeps it to Counter
    /// Siege).
    pub fn refuses(&self, mode: Mode) -> Option<String> {
        let dojo = Mode::made_for(self) == Mode::KungFu;
        match mode {
            Mode::KungFu if !dojo => Some(format!("{} is not a Kung Fu map", self.name)),
            Mode::KungFu | Mode::Cards => None,
            _ if dojo => Some(format!("{} is a Kung Fu map", self.name)),
            // Titan needs the titan layout (a map tagged `titan`: titan.rs, the interface's section 1).
            Mode::Titan if self.mode != crate::titan::MODE => Some(format!("{} has no Titan layout", self.name)),
            Mode::Siege if self.spawns.is_empty() => Some(format!("{} has no cat spawn point", self.name)),
            Mode::Forts if self.fortress().is_none() => Some(format!("{} has no Forts layout (two bases, a field and a bacon spot)", self.name)),
            _ => None,
        }
    }
    /// The modes this map can be played in, in `Mode::ALL`'s order.
    pub fn modes(&self) -> Vec<Mode> {
        Mode::ALL.into_iter().filter(|m| self.refuses(*m).is_none()).collect()
    }
}

/// Free For All: kills to win, seconds a round lasts, seconds until a dead hotdog is back, and its spawn protection.
pub const FFA_KILLS: u32 = 25;
pub const FFA_TIME: f32 = 300.;
pub const FFA_RESPAWN: f32 = 5.;
pub const SHIELD: f32 = 1.;
/// The sandbox: seconds until a dead hotdog is back.
pub const SANDBOX_RESPAWN: f32 = 0.5;
/// The guns by number as the rules (and the tuning) name them; 0 is the melee strike.
pub const WEAPONS: [&str; GUNS] = ["melee", "mustard", "sniper", "hotsauce", "relish", "bugspray", "syrup", "mayo"];
/// Other names a gun goes by.
const ALIASES: [(&str, usize); 9] = [("pistol", 1), ("ketchup", 2), ("flame", 3), ("thrower", 3), ("bazooka", 4), ("spray", 5), ("rifle", 6), ("mayonnaise", 7), ("bottle", 7)];

/// A gun by name or number (1 to the last gun), or None.
pub fn weapon(name: &str) -> Option<usize> {
    let name = name.trim().to_ascii_lowercase();
    if let Ok(n) = name.parse::<usize>() {
        return (1..GUNS).contains(&n).then_some(n);
    }
    WEAPONS.iter().position(|w| *w == name).filter(|w| *w > 0).or_else(|| ALIASES.iter().find(|(a, _)| *a == name).map(|(_, w)| *w))
}

/// What the sandbox's bots do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BotMode {
    /// Stand still at their start.
    Idle,
    /// Walk to and fro across their start.
    Walk,
    /// Play as bots do.
    #[default]
    Fight,
}
impl BotMode {
    fn named(name: &str) -> Option<BotMode> {
        match name {
            "idle" | "still" | "stand" => Some(BotMode::Idle),
            "walk" | "move" | "patrol" => Some(BotMode::Walk),
            "fight" | "play" | "normal" => Some(BotMode::Fight),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            BotMode::Idle => "idle",
            BotMode::Walk => "walk",
            BotMode::Fight => "fight",
        }
    }
}

/// See the module's documentation. The default is a game as it always was: the mode the map was made for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rules {
    /// The siege or Forts, asked for by name (`mode=siege`, `mode=forts`): a Forts map played as a siege, or a map with
    /// the Forts layout played as Forts. Neither: the map's own mode.
    pub siege: bool,
    pub forts: bool,
    pub ffa: bool,
    /// Free For All's rounds run out (magazine.rs), as every gun mode's do: `mode=ffa` sets it, and `ammo=inf` with it
    /// clears it (rounds never run out). It means nothing outside Free For All: the siege's and Forts' rounds always run
    /// out, and the sandbox's never do with its own `ammo=inf`.
    pub finite_ammo: bool,
    /// Fully Loaded (cards.rs): the hotdogs play a card game at a table on the microwave.
    pub cards: bool,
    /// Kung Fu (kungfu.rs): hands and feet only.
    pub kungfu: bool,
    /// Titan (titan.rs): one giant hotdog against the rest; who is to be the titan (`titan=`), and how many small hotdogs
    /// (`smalls=`, None for titan::SMALLS).
    pub titan: bool,
    pub pick: crate::titan::Pick,
    pub smalls: Option<usize>,
    pub sandbox: bool,
    /// The sandbox's loadout for a person's hotdog (empty: the usual guns).
    pub weapons: Vec<usize>,
    pub ammo: bool,
    pub god: bool,
    pub bots: Option<usize>,
    pub botmode: BotMode,
    pub zombies: bool,
    pub field: bool,
    pub stim: bool,
    pub here: bool,
    /// How well the bots aim (aim.rs): normal unless asked.
    pub skill: crate::aim::Skill,
}
/// Whether a value says yes: a bare key, or on, inf, true, 1...
fn yes(value: &str) -> bool {
    !matches!(value, "0" | "off" | "no" | "false" | "none")
}
impl Rules {
    /// The rules a list of keys and values asks for (an address's query, a join's `sandbox`): see the module's
    /// documentation. Unknown keys and values are passed over; without `sandbox` only `mode=` (and `ammo=inf` with
    /// `mode=ffa`, and `skill=`) count.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Rules {
        let pairs: Vec<(String, String)> = pairs.into_iter().map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase())).collect();
        let get = |key: &str| pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let mode = get("mode").and_then(Mode::named);
        let mut rules = Rules { sandbox: get("sandbox").is_some_and(yes), ..Rules::of(mode) };
        // (Free For All's rounds run out, unless `ammo=inf` says otherwise.)
        rules.finite_ammo = rules.ffa && !get("ammo").is_some_and(yes);
        // The bots' skill, in any game.
        if let Some(skill) = get("skill").or(get("difficulty")).or(get("bots")).and_then(crate::aim::Skill::named) {
            rules.skill = skill;
        }
        // Titan: who is the titan, and how many small hotdogs (in any game of it).
        if rules.titan {
            rules.pick = get("titan").and_then(crate::titan::Pick::named).unwrap_or_default();
            rules.smalls = get("smalls").and_then(|n| n.parse::<usize>().ok()).map(|n| n.clamp(crate::titan::SMALLS_MIN, crate::titan::SMALLS_MAX));
        }
        if !rules.sandbox {
            return rules;
        }
        if let Some(list) = get("weapons").or(get("weapon")) {
            for name in list.split([',', '+', ' ']).filter(|n| !n.is_empty()) {
                if name == "all" {
                    rules.weapons = (1..GUNS).collect();
                } else if let Some(w) = weapon(name).filter(|w| !rules.weapons.contains(w)) {
                    rules.weapons.push(w);
                }
            }
        }
        rules.ammo = get("ammo").is_some_and(yes);
        rules.god = get("god").is_some_and(yes);
        if let Some(bots) = get("bots") {
            match (bots.parse::<usize>(), BotMode::named(bots)) {
                (Ok(n), _) => rules.bots = Some(n.min(5)),
                (_, Some(mode)) => rules.botmode = mode,
                _ => {}
            }
        }
        if let Some(mode) = get("botmode").and_then(BotMode::named) {
            rules.botmode = mode;
        }
        rules.zombies = get("zombies").is_some_and(yes);
        rules.field = get("field").is_some_and(yes);
        rules.stim = get("stim").is_some_and(yes);
        rules.here = get("respawn") == Some("here");
        rules
    }
    /// The rules `text` says (as `text` writes them, or an address's query: `key` or `key=value`, apart by spaces,
    /// ampersands or semicolons).
    pub fn parse(text: &str) -> Rules {
        Rules::from_pairs(text.split([' ', '&', ';', '\n', '?']).filter(|t| !t.is_empty()).map(|t| t.split_once('=').unwrap_or((t, ""))))
    }
    /// The rules as one line of `key` and `key=value` (empty for a game as it always was).
    pub fn text(&self) -> String {
        let mut out = vec![];
        if self.sandbox {
            out.push("sandbox".to_string());
        }
        if let Some(mode) = self.mode() {
            out.push(format!("mode={}", mode.name()));
        }
        // (Free For All with its rounds never running out, and the sandbox's unlimited rounds, say the same.)
        let endless = self.ffa && !self.finite_ammo;
        if self.skill != crate::aim::Skill::Normal {
            out.push(format!("skill={}", self.skill.name()));
        }
        if self.titan {
            if self.pick != crate::titan::Pick::Auto {
                out.push(format!("titan={}", self.pick.name()));
            }
            if let Some(n) = self.smalls {
                out.push(format!("smalls={n}"));
            }
        }
        if !self.sandbox {
            if endless {
                out.push("ammo=inf".into());
            }
            return out.join(" ");
        }
        if !self.weapons.is_empty() {
            out.push(format!("weapons={}", self.weapons.iter().map(|w| WEAPONS[*w]).collect::<Vec<_>>().join(",")));
        }
        for (on, word) in [(self.ammo || endless, "ammo=inf"), (self.god, "god")] {
            if on {
                out.push(word.into());
            }
        }
        if let Some(n) = self.bots {
            out.push(format!("bots={n}"));
        }
        if self.botmode != BotMode::Fight {
            out.push(format!("botmode={}", self.botmode.name()));
        }
        for (on, word) in [(self.zombies, "zombies=on"), (self.field, "field=on"), (self.stim, "stim=inf"), (self.here, "respawn=here")] {
            if on {
                out.push(word.into());
            }
        }
        out.join(" ")
    }
    /// The rules of a game of `mode` (None: the map's own), as nothing else asked for: Free For All's rounds run out.
    pub fn of(mode: Option<Mode>) -> Rules {
        let is = |m: Mode| mode == Some(m);
        Rules { siege: is(Mode::Siege), forts: is(Mode::Forts), ffa: is(Mode::Ffa), finite_ammo: is(Mode::Ffa), cards: is(Mode::Cards), kungfu: is(Mode::KungFu), titan: is(Mode::Titan), ..Default::default() }
    }
    /// The rules of a game of `mode` on `map`: as `of`, but naming no mode where it is the one the map was made for, so
    /// that a game in a map's own mode says what it always did (an empty line for a siege or Forts).
    pub fn playing(mode: Mode, map: &Map) -> Rules {
        let own = Mode::made_for(map) == mode && matches!(mode, Mode::Siege | Mode::Forts);
        Rules::of(Some(mode).filter(|_| !own))
    }
    /// The mode the rules ask for by name (None: the map's own). Of two (a struct written by hand), the first in
    /// `Mode::ALL`'s order but the siege: Free For All, Fully Loaded and Kung Fu, as the game plays them, go first.
    pub fn mode(&self) -> Option<Mode> {
        [(self.ffa, Mode::Ffa), (self.cards, Mode::Cards), (self.kungfu, Mode::KungFu), (self.titan, Mode::Titan), (self.forts, Mode::Forts), (self.siege, Mode::Siege)].into_iter().find(|(on, _)| *on).map(|(_, m)| m)
    }
    /// Whether the game is played by rules of its own beyond a siege's or Forts' (Free For All, the sandbox, Kung Fu:
    /// respawns, their own end); the siege and Forts, named or not, are not.
    pub fn active(&self) -> bool {
        self.ffa || self.sandbox || self.kungfu || self.titan
    }
    /// Whether rounds never run out: the sandbox's `ammo=inf`, or Free For All without `finite_ammo`.
    pub fn unlimited(&self) -> bool {
        ((self.ffa && !self.finite_ammo) || (self.sandbox && self.ammo)) && !self.kungfu
    }
    /// How many small hotdogs a game of Titan has.
    pub fn small_count(&self) -> usize {
        self.smalls.unwrap_or(crate::titan::SMALLS).clamp(crate::titan::SMALLS_MIN, crate::titan::SMALLS_MAX)
    }
    /// Whether the sandbox's cats are out.
    pub fn roaming(&self) -> bool {
        self.sandbox && self.zombies
    }
    /// Whether a round ends (Free For All's, by kills or the clock; the sandbox's never does).
    pub fn ends(&self) -> bool {
        (self.ffa || self.kungfu || self.titan) && !self.sandbox
    }
    /// Kills (Kung Fu: KOs) that win a round that ends.
    pub fn goal(&self) -> u32 {
        if self.kungfu { crate::kungfu::KOS } else { FFA_KILLS }
    }
    /// Seconds a round that ends lasts.
    pub fn time(&self) -> f32 {
        if self.kungfu {
            crate::kungfu::TIME
        } else if self.titan {
            crate::titan::TIME
        } else {
            FFA_TIME
        }
    }
    /// The line at the top of the screen as a round begins.
    pub fn banner(&self) -> String {
        if self.titan {
            return match self.sandbox {
                true => "SANDBOX: TITAN. NO CLOCK, NO END.".into(),
                false => format!("TITAN. BRING THE GIANT DOWN: AIM FOR THE BACK OF ITS HEAD. {} MINUTES.", crate::titan::TIME as u32 / 60),
            };
        }
        if self.kungfu {
            return match self.sandbox {
                true => "SANDBOX: KUNG FU. NO CLOCK, NO END.".into(),
                false => format!("KUNG FU. HANDS AND FEET ONLY. FIRST TO {} KOS, OR THE MOST IN {} MINUTES.", crate::kungfu::KOS, crate::kungfu::TIME as u32 / 60),
            };
        }
        match (self.sandbox, self.ffa) {
            (true, true) => "SANDBOX: FREE FOR ALL. NO CLOCK, NO END.".into(),
            (true, false) => "SANDBOX. NO CLOCK, NO WAVE, NO END: TRY THINGS OUT.".into(),
            _ => format!("FREE FOR ALL. FIRST TO {FFA_KILLS} KILLS, OR THE MOST IN {} MINUTES.", FFA_TIME as u32 / 60),
        }
    }
}

/// Rounds that never run out: every gun the hotdog holds with its magazine and its reserve full (magazine.rs).
fn fill(p: &mut crate::feast::Player) {
    for w in 1..GUNS {
        if p.owned[w] {
            (p.ammo[w], p.reserve[w]) = (p.ammo[w].max(FULL[w]), p.reserve[w].max(crate::magazine::CARRY[w]));
        }
    }
}

impl Feast {
    /// Plays the round by `rules` from now on (and every round after it): see the module's documentation. Called on a
    /// round just set out (and online on a client's copy of it whenever the server's rules change); the usual rules on
    /// a map's own mode leave it as it is. The mode is the rules', or the map's own: Forts where the map has the Forts
    /// layout (the round set out with its teams and forcefield, `fortify`, unless it already is), and otherwise none of
    /// Forts (`unfort`: a Forts map played as anything else).
    pub fn set_rules(&mut self, mut rules: Rules) {
        // A Kung Fu map (the dojo) is played as Kung Fu whatever the rules say.
        rules.kungfu |= self.level.map.mode == crate::kungfu::MODE;
        // A titan arena is played as Titan when the rules name no mode (titan.rs).
        rules.titan |= rules.mode().is_none() && self.level.map.mode == crate::titan::MODE;
        self.rules = rules;
        self.ends = 0.;
        // (A table only stands while the rules say so; nor does a dojo, nor a titan.)
        self.cards = None;
        self.kungfu = None;
        if self.titan.is_some() {
            self.titan_previous = self.titan_last();
            self.titan = None;
            self.untitan();
        }
        if self.rules.titan {
            self.unfort();
            return self.open_titan();
        }
        if self.rules.kungfu {
            self.unfort();
            return self.open_dojo();
        }
        if self.rules.cards {
            // Forts' bases, forcefield and bacon have no place at a table: five hotdogs, as in Free For All.
            self.unfort();
            return self.lay_table();
        }
        // Forts, where it is asked for (or the map was made for it) and the map has its layout; otherwise none of it.
        let forts = match self.rules.mode() {
            Some(mode) => mode == Mode::Forts,
            None => Mode::made_for(&self.level.map) == Mode::Forts,
        } && self.level.map.refuses(Mode::Forts).is_none();
        if !forts {
            self.unfort();
        } else if self.fort.is_none() {
            self.fortify();
        }
        if !self.rules.active() {
            return;
        }
        // Free For All leaves a Forts map's zones and forcefield out, and spreads the hotdogs over the map (spawns.rs).
        self.openings.clear();
        if self.rules.ffa {
            self.unfort();
            self.open();
        }
        if self.fort.is_some() {
            // The sandbox on a Forts map: no build phase; the forcefield is down unless asked for, and the guns are out.
            if !self.rules.field {
                self.lower_field();
            }
            self.stage = Stage::Fight;
            for item in &mut self.pickups {
                item.ready = 0.;
            }
            for p in &mut self.players {
                p.ready = false;
            }
        } else {
            // A siege map: everyone against everyone from the start (the bots do not wait for the famine to sink in).
            self.stage = Stage::FreeForAll;
            self.famine_at = self.elapsed - 5.;
        }
        if self.rules.ends() {
            self.ends = self.elapsed + self.rules.time();
        }
        self.alert = self.rules.banner();
        let ids: Vec<u32> = self.players.iter().map(|p| p.id).collect();
        for id in ids {
            self.equip(id);
        }
        self.bench();
        self.embody();
    }
    /// The mode the round is played in: the table's, the dojo's, Free For All's, Forts' while its state stands, and
    /// otherwise the siege's.
    pub fn mode(&self) -> Mode {
        if self.cards.is_some() || self.rules.cards {
            Mode::Cards
        } else if self.kungfu.is_some() || self.rules.kungfu {
            Mode::KungFu
        } else if self.titan.is_some() || self.rules.titan {
            Mode::Titan
        } else if self.rules.ffa {
            Mode::Ffa
        } else if self.fort.is_some() {
            Mode::Forts
        } else {
            Mode::Siege
        }
    }
    /// The siege's famine: its free for all after the wave, where hunger bites and the hotdogs may bite and eat each other
    /// (Free For All, which shares its stage, has none of that).
    pub fn famine(&self) -> bool {
        self.stage == Stage::FreeForAll && !self.rules.ffa && self.kungfu.is_none() && self.cards.is_none() && self.titan.is_none()
    }
    /// A Forts round becomes another mode's (Free For All, a siege, a table, a dojo): the forcefield, the bases and the
    /// bacon go, and the five hotdogs of a siege stand at the map's starts (until Free For All's opening spreads them,
    /// spawns.rs), with the siege's warm-up to come. Nothing happens on a round that is not Forts.
    fn unfort(&mut self) {
        if self.fort.is_none() {
            return;
        }
        self.lower_field();
        self.fort = None;
        self.relevel();
        self.players.truncate(NAMES.len());
        for p in &mut self.players {
            (p.team, p.job) = (0, Default::default());
        }
        for id in 1..=self.players.len() as u32 {
            self.fit(id);
        }
        for item in &mut self.pickups {
            item.ready = 0.;
        }
        // (The siege's opening, as a new round sets it out: Free For All, a table and a dojo set their own.)
        (self.stage, self.famine_at, self.wave_spawned, self.wave_kills) = (Stage::Warmup, 0., 0, 0);
        self.warmup = self.elapsed + if self.demo { 3. } else { 12. };
        self.alert = crate::feast::SIEGE_ALERT.into();
    }
    /// A hotdog as it starts or comes back, beyond the loadout every gun mode gives it (`fit`, magazine.rs `loadout`):
    /// the sandbox's loadout for a person's (full magazines and their spare rounds), and full guns while rounds never
    /// run out.
    pub(crate) fn equip(&mut self, id: u32) {
        // Kung Fu: no guns; the six styles are what a hotdog "holds" (kungfu.rs).
        if self.kungfu.is_some() {
            return self.arm_for_kungfu(id);
        }
        // Titan: the titan holds no gun (titan.rs).
        if self.is_titan(id) {
            return self.disarm_titan(id);
        }
        let loadout: Vec<usize> = if self.rules.sandbox { self.rules.weapons.iter().copied().filter(|w| (1..GUNS).contains(w)).collect() } else { vec![] };
        let unlimited = self.rules.unlimited();
        let Some(p) = self.player_mut(id) else {
            return;
        };
        let given = loadout.first().copied().filter(|_| !p.bot);
        if let Some(first) = given {
            (p.owned, p.ammo, p.reserve, p.reload) = ([false; GUNS], [0; GUNS], [0; GUNS], None);
            p.ammo[0] = -1;
            for w in loadout {
                p.owned[w] = true;
            }
            (p.weapon, p.gun) = (first, first);
        }
        if unlimited {
            fill(p);
        } else if given.is_some() {
            self.spare(id);
        }
    }
    /// God mode (the sandbox, a person's hotdog) and spawn protection: a hit that takes nothing. It goes on the
    /// hotdog's record all the same (with what it would have taken), so that it shows. A fall is never spared.
    pub(crate) fn spared(&mut self, id: u32, amount: f32, rival: bool, cause: &Damage) -> bool {
        let (time, god, stimmed) = (self.elapsed, self.rules.sandbox && self.rules.god, self.tune.stim_damage);
        let Some(p) = self.player_mut(id).filter(|p| p.alive) else {
            return false;
        };
        if cause.kind == FALL || !((god && !p.bot) || p.shield > 0.) {
            return false;
        }
        let scale = if rival && p.stim > 0. { stimmed } else { 1. };
        p.last_damage = Damage { time, amount: (amount * scale).min(p.hp), ..*cause };
        true
    }
    /// A hotdog has died: in every mode its killer, if a hotdog killed it, is credited with the kill (a kill and 100
    /// points); in Free For All, the sandbox and Kung Fu it is counted a death and will be back.
    pub(crate) fn mourn(&mut self, id: u32) {
        let killer = self.player(id).map_or(0, |p| p.last_damage.attacker);
        if killer != 0 && killer != id {
            if let Some(k) = self.player_mut(killer) {
                (k.kills, k.score) = (k.kills + 1, k.score + 100);
            }
        }
        if !self.rules.active() {
            return;
        }
        // Titan: the titan does not come back, and a small hotdog only from the pool (titan.rs).
        if self.titan.is_some() {
            return self.titan_mourn(id);
        }
        let delay = if self.rules.sandbox { SANDBOX_RESPAWN } else if self.rules.kungfu { crate::kungfu::RESPAWN } else { FFA_RESPAWN };
        if let Some(p) = self.player_mut(id) {
            (p.deaths, p.respawn, p.shield) = (p.deaths + 1, delay, 0.);
        }
    }
    /// One step of Free For All's and the sandbox's own rules, after everything else has moved: spawn protection wears
    /// off (at once once its hotdog has fired or struck), rounds and stims are topped up, the sandbox's bench is kept,
    /// the dead come back when their time is up, the sandbox's cats come out again, and Free For All's round ends.
    pub(crate) fn referee(&mut self, dt: f32) {
        if !self.rules.active() {
            return;
        }
        let (unlimited, stim, stim_time) = (self.rules.unlimited(), self.rules.sandbox && self.rules.stim, self.tune.stim_time);
        for p in &mut self.players {
            p.shield = if p.cooldown > 0. { 0. } else { (p.shield - dt).max(0.) };
            if unlimited {
                fill(p);
            }
            if stim && p.alive {
                p.stim_wait = 0.;
                if p.stim > 0. {
                    p.stim = stim_time;
                }
            }
        }
        self.bench();
        let mut due = vec![];
        for p in self.players.iter_mut().filter(|p| !p.alive && !p.benched && p.respawn > 0.) {
            p.respawn -= dt;
            if p.respawn <= 1e-6 {
                p.respawn = 0.;
                due.push(p.id);
            }
        }
        for id in due {
            self.revive(id);
        }
        if self.rules.roaming() && self.enemies.is_empty() && self.pending() == 0 {
            self.emerge(self.wave());
        }
        if self.rules.ends() && self.stage != Stage::RoundOver {
            self.final_whistle();
        }
    }
    /// A dead hotdog comes back now: whole, with the guns it starts with, where the rules say (Free For All: a safe
    /// spot out of the living's sight, spawns.rs; the sandbox: its start, or with respawn=here where it fell), its
    /// kills, deaths and score kept. Free For All gives it its spawn protection.
    pub fn revive(&mut self, id: u32) {
        let Some(old) = self.player(id).cloned() else {
            return;
        };
        let at = if self.rules.kungfu {
            Some(self.dojo_spot(id))
        } else if self.titan.is_some() {
            Some(self.titan_haven(id))
        } else if self.rules.ffa {
            Some(self.haven(id))
        } else if self.rules.here && !old.fallen {
            Some((Vec3::from_array(old.p), old.yaw))
        } else {
            None
        };
        self.fit(id);
        let shield = if self.rules.ffa || self.rules.kungfu || self.titan.is_some() { SHIELD } else { 0. };
        let Some(p) = self.player_mut(id) else {
            return;
        };
        (p.kills, p.deaths, p.score, p.meals, p.swings, p.name, p.shield) = (old.kills, old.deaths, old.score, old.meals, old.swings, old.name, shield);
        if let Some((at, yaw)) = at {
            (p.p, p.yaw, p.camp) = (at.to_array(), yaw, [at.x, at.z]);
            if self.rules.ffa {
                self.came_back.insert(id, glam::Vec2::new(at.x, at.z));
            }
            self.embody();
        }
        if self.titan.is_some() {
            self.titan_back(id);
        }
    }
    /// The sandbox's `bots=N`: the first N bots (by seat) play; the rest are benched (dead, eaten, nowhere to be seen)
    /// until a seat frees up for them.
    fn bench(&mut self) {
        let Some(n) = self.rules.bots.filter(|_| self.rules.sandbox) else {
            return;
        };
        let bots: Vec<usize> = (0..self.players.len()).filter(|i| self.players[*i].bot).collect();
        let mut back = vec![];
        for (k, i) in bots.into_iter().enumerate() {
            let p = &mut self.players[i];
            if k >= n && !p.benched {
                if let Ok(prop) = usize::try_from(p.carry) {
                    if prop < self.props.len() {
                        self.props.drop_prop(prop, [0.; 3], [0.; 3]);
                    }
                }
                (p.benched, p.alive, p.consumed, p.hp, p.respawn, p.carry) = (true, false, true, 0., 0., -1);
            } else if k < n && p.benched {
                p.benched = false;
                back.push(p.id);
            }
        }
        for id in back {
            self.revive(id);
        }
        self.players.iter_mut().filter(|p| !p.bot && p.benched).for_each(|p| p.benched = false);
    }
    /// The sandbox's bots when they are targets (botmode idle or walk): returns whether it has seen to the bot.
    pub(crate) fn drill(&mut self, id: u32, dt: f32) -> bool {
        if !self.rules.sandbox || self.rules.botmode == BotMode::Fight {
            return false;
        }
        if self.rules.botmode == BotMode::Walk {
            let Some(p) = self.player(id) else {
                return true;
            };
            // To and fro along a line across where it starts, a leg a second.
            let (anchor, yaw) = match &self.fort {
                Some(_) => {
                    let rank = self.players.iter().filter(|q| q.team == p.team && q.id < id).count();
                    let (at, yaw) = self.level.team_start(p.team as usize, rank);
                    (Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32), yaw as f32)
                }
                None => self.level.start((id as usize).saturating_sub(1)),
            };
            let side = if (self.elapsed * 0.5 + id as f32 * 0.37).fract() < 0.5 { 1. } else { -1. };
            let goal = anchor + Vec3::new(yaw.cos(), 0., -yaw.sin()) * 0.3 * side;
            self.advance_actor(id, goal, dt);
        }
        true
    }
    /// Free For All: the round ends with FFA_KILLS kills, or when its time is up (whoever has most kills wins; a tie
    /// at the top is a draw). The line at the top of the screen is the score.
    fn final_whistle(&mut self) {
        // Titan has an end of its own (titan.rs).
        if self.titan.is_some() {
            return self.titan_whistle();
        }
        let top = self.players.iter().map(|p| p.kills).max().unwrap_or(0);
        let leaders: Vec<u32> = self.players.iter().filter(|p| p.kills == top).map(|p| p.id).collect();
        let timed = self.ends > 0. && self.elapsed >= self.ends;
        if top < self.rules.goal() && !timed {
            return;
        }
        self.stage = Stage::RoundOver;
        self.over_at = self.elapsed;
        for q in &mut self.projectiles {
            q.hostile = false;
        }
        self.winner = (leaders.len() == 1).then(|| leaders[0]);
        let what = if self.rules.kungfu { "KOS" } else { "KILLS" };
        let headline = match self.winner.and_then(|id| self.player(id)) {
            Some(p) if timed => format!("TIME. {} WINS WITH {} {what}", p.name, p.kills),
            Some(p) => format!("{} WINS WITH {} {what}", p.name, p.kills),
            None => format!("TIME. A DRAW AT {top} {what}"),
        };
        let mut table: Vec<&crate::feast::Player> = self.players.iter().filter(|p| !p.benched).collect();
        table.sort_by(|a, b| b.kills.cmp(&a.kills).then(a.deaths.cmp(&b.deaths)).then(a.id.cmp(&b.id)));
        let board: Vec<String> = table.iter().map(|p| format!("{} {}/{}", p.name, p.kills, p.deaths)).collect();
        self.alert = format!("{headline}. {}", board.join(" · "));
    }
    /// Free For All: seconds left of the round (None without a clock).
    pub fn time_left(&self) -> Option<f32> {
        (self.ends > 0.).then(|| (self.ends - self.elapsed).max(0.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feast::{CHEST, PLAYER_DAMAGE};
    use crate::map::Map;

    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    fn siege(rules: &str) -> Feast {
        let mut s = Feast::new(false);
        s.set_rules(Rules::parse(rules));
        s
    }
    fn forts(rules: &str) -> Feast {
        let mut s = Feast::with_map(Map::parse(&read("assets/maps/pillow-fort.json")).unwrap(), false);
        s.set_rules(Rules::parse(rules));
        s
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 60.).round() as usize {
            s.tick(1. / 60.);
        }
    }
    /// A round of mustard from `from` straight at `to`'s chest, as if fired (the shooter's hit is the rules' to judge).
    fn shoot(s: &mut Feast, from: u32, to: u32, weapon: usize) {
        let o = crate::feast::muzzle(s.player(from).unwrap());
        let t = Vec3::from_array(s.player(to).unwrap().p) + Vec3::Y * CHEST[0];
        let d = (t - o).normalize();
        let id = s.launch(from, o, d, weapon);
        let _ = id;
    }
    fn put(s: &mut Feast, id: u32, x: f32, z: f32) {
        s.player_mut(id).unwrap().p = [x, crate::feast::FLOOR, z];
        s.embody();
    }
    /// Hotdog 1 faces hotdog 2 from 0.3 m on open counter; the rest are parked out of the way (as feast.rs `duel`).
    fn face_off(s: &mut Feast) {
        for id in 3..=s.players.len() as u32 {
            put(s, id, -1.5 + id as f32 * 0.1, 3.55);
        }
        put(s, 1, 0., 2.4);
        put(s, 2, 0., 2.7);
    }
    /// Nobody plays by the bots' rules: they stand where they are put.
    fn people(s: &mut Feast) {
        for p in &mut s.players {
            p.bot = false;
        }
    }

    #[test]
    fn rules_are_read_from_an_address_or_a_line_and_written_back_the_same() {
        let r = Rules::parse("?mp&sandbox&weapons=mustard,sniper,9,spray&ammo=inf&god&bots=2&botmode=idle&zombies=on&field=on&stim=inf&respawn=here&mode=ffa&map=jungle-fort");
        assert!(r.sandbox && r.ffa && r.ammo && r.god && r.zombies && r.field && r.stim && r.here);
        assert_eq!((r.weapons.clone(), r.bots, r.botmode), (vec![1, 2, 5], Some(2), BotMode::Idle));
        assert_eq!(Rules::parse(&r.text()), r, "{}", r.text());
        assert_eq!(r.text(), "sandbox mode=ffa weapons=mustard,sniper,bugspray ammo=inf god bots=2 botmode=idle zombies=on field=on stim=inf respawn=here");
        // Names and numbers alike, and all of them.
        assert_eq!(Rules::parse("sandbox weapons=pistol,2,flame,bazooka,rifle,mayo").weapons, vec![1, 2, 3, 4, 6, 7]);
        assert_eq!(Rules::parse("sandbox weapons=all").weapons, (1..GUNS).collect::<Vec<_>>());
        assert_eq!(Rules::parse("sandbox bots=walk").botmode, BotMode::Walk);
        // Without the sandbox only Free For All counts; with nothing, the usual game.
        assert_eq!(Rules::parse("god weapons=all mode=ffa"), Rules { ffa: true, finite_ammo: true, ..Default::default() });
        assert_eq!(Rules::parse(""), Rules::default());
        assert_eq!(Rules::default().text(), "");
        assert!(!Rules::parse("sandbox=0 god").sandbox);
        assert_eq!(Rules::from_pairs([("sandbox", ""), ("zombies", "off"), ("ammo", "inf")]), Rules { sandbox: true, ammo: true, ..Default::default() });
    }

    #[test]
    fn mode_cards_is_read_and_written() {
        // On its own, from an address's query or a line, and written back the same.
        let r = Rules::parse("?mp&mode=cards&map=counter-siege");
        assert_eq!(r, Rules { cards: true, ..Default::default() });
        assert_eq!(r.text(), "mode=cards");
        assert_eq!(Rules::parse(&r.text()), r);
        // With or without the sandbox, as Free For All's is.
        let boxed = Rules::parse("sandbox mode=cards god");
        assert!(boxed.cards && boxed.sandbox && !boxed.ffa && boxed.god);
        assert!(boxed.text().starts_with("sandbox mode=cards"), "{}", boxed.text());
        assert_eq!(Rules::parse(&boxed.text()), boxed);
        // `mode=ffa` still gives Free For All only (its rounds running out), and no other mode gives cards.
        assert_eq!(Rules::parse("mode=ffa"), Rules { ffa: true, finite_ammo: true, ..Default::default() });
        assert_eq!(Rules::parse("mode=ffa").text(), "mode=ffa");
        for line in ["", "mode=siege", "mode=forts", "mode=card", "cards", "sandbox"] {
            assert!(!Rules::parse(line).cards, "{line}");
        }
        assert!(!Rules::default().cards);
    }

    #[test]
    fn a_game_with_no_rules_is_as_it_always_was() {
        let (mut plain, mut ruled) = (Feast::new(false), Feast::new(false));
        ruled.set_rules(Rules::default());
        run(&mut plain, 3.);
        run(&mut ruled, 3.);
        assert_eq!((plain.stage, ruled.stage), (Stage::Warmup, Stage::Warmup));
        assert_eq!(plain.players.iter().map(|p| p.p).collect::<Vec<_>>(), ruled.players.iter().map(|p| p.p).collect::<Vec<_>>());
        assert_eq!(ruled.ends, 0.);
    }

    #[test]
    fn the_sandbox_never_changes_phase_has_no_clock_no_hunger_no_wave_and_never_ends() {
        let mut s = siege("sandbox bots=0");
        assert_eq!(s.stage, Stage::FreeForAll);
        assert_eq!(s.time_left(), None);
        let hp = s.player(1).unwrap().hp;
        run(&mut s, 40.);
        assert_eq!(s.stage, Stage::FreeForAll, "no round end with one hotdog left");
        assert_eq!(s.player(1).unwrap().hp, hp, "no hunger");
        assert!(s.enemies.is_empty() && s.pending() == 0 && s.pests.is_empty(), "no wave, no pests");
        // Forts: straight into the fight with the field down, the guns out, no stalemate cats; field=on keeps it up.
        let mut f = forts("sandbox");
        assert_eq!(f.stage, Stage::Fight);
        assert!(!f.fort.as_ref().unwrap().up && f.pickups.iter().all(|p| p.ready <= f.elapsed));
        run(&mut f, 50.);
        assert_eq!(f.stage, Stage::Fight);
        assert!(f.enemies.is_empty() && f.pending() == 0, "no stalemate");
        let f = forts("sandbox field=on");
        assert!(f.fort.as_ref().unwrap().up && f.stage == Stage::Fight);
    }

    #[test]
    fn the_sandbox_loadout_is_what_a_person_starts_and_respawns_with_and_rounds_never_run_out() {
        let mut s = siege("sandbox weapons=mustard,sniper ammo=inf bots=0");
        let me = s.player(1).unwrap().clone();
        assert_eq!((me.owned[1], me.owned[2], me.owned.iter().filter(|o| **o).count(), me.weapon), (true, true, 2, 1));
        assert_eq!((me.ammo[1], me.ammo[2]), (FULL[1], FULL[2]));
        put(&mut s, 1, 0., 2.4);
        let d = Vec3::Z;
        for _ in 0..40 {
            s.fire(1, d, 2);
            run(&mut s, 0.2);
        }
        assert!(s.player(1).unwrap().ammo[2] >= FULL[2], "unlimited");
        // All the guns: every one held.
        let all = siege("sandbox weapons=all");
        assert!((1..GUNS).all(|w| all.player(1).unwrap().owned[w]));
        // A death and the respawn give the loadout back (with no bots to shoot it as it comes back).
        let mut s = siege("sandbox weapons=sniper,relish bots=0");
        s.hurt(1, 500.);
        assert!(!s.player(1).unwrap().alive);
        run(&mut s, SANDBOX_RESPAWN + 0.1);
        let back = s.player(1).unwrap();
        assert!(back.alive && back.hp == 100. && back.owned[2] && back.owned[4] && !back.owned[1] && back.weapon == 2);
        assert_eq!(back.deaths, 1);
        // fit, as the server calls it for a hotdog taken over, gives it too.
        s.fit(1);
        assert!(s.player(1).unwrap().owned[4]);
    }

    #[test]
    fn god_mode_takes_no_hp_from_a_person_but_the_hit_is_on_its_record_and_bots_still_die() {
        let mut s = siege("sandbox god botmode=idle");
        face_off(&mut s);
        shoot(&mut s, 2, 1, 2);
        run(&mut s, 0.5);
        let me = s.player(1).unwrap();
        assert_eq!(me.hp, 100.);
        assert!(me.last_damage.attacker == 2 && me.last_damage.amount > 0., "the hit shows: {:?}", me.last_damage);
        shoot(&mut s, 1, 2, 2);
        run(&mut s, 0.3);
        assert!(s.player(2).unwrap().hp <= 100. - PLAYER_DAMAGE[2] + 1e-3, "a bot is hurt");
    }

    #[test]
    fn the_sandbox_respawns_at_the_start_or_where_it_fell_and_benches_bots_beyond_the_count() {
        let mut s = siege("sandbox bots=2 botmode=idle");
        let bots: Vec<bool> = s.players.iter().map(|p| p.alive).collect();
        assert_eq!(bots, vec![true, true, true, false, false], "two bots play, two are benched");
        assert!(s.players[3].consumed && s.players[3].benched);
        run(&mut s, 5.);
        assert!(!s.players[4].alive, "benched hotdogs stay down");
        let start = s.level.start(1).0;
        put(&mut s, 2, 0., 2.7);
        s.hurt(2, 500.);
        run(&mut s, SANDBOX_RESPAWN + 0.05);
        assert!(s.player(2).unwrap().alive);
        assert!(Vec3::from_array(s.player(2).unwrap().p).distance(start) < 0.03, "back at its start");
        let mut s = siege("sandbox respawn=here bots=1 botmode=idle");
        put(&mut s, 1, 0., 2.6);
        s.hurt(1, 500.);
        run(&mut s, SANDBOX_RESPAWN + 0.05);
        let p = s.player(1).unwrap().p;
        assert!(s.player(1).unwrap().alive && p[0].abs() < 0.03 && (p[2] - 2.6).abs() < 0.03, "{p:?}");
        // Idle bots stand; walking bots move to and fro.
        let mut idle = siege("sandbox botmode=idle");
        let before = idle.player(3).unwrap().p;
        run(&mut idle, 3.);
        assert_eq!(idle.player(3).unwrap().p, before);
        let mut walk = siege("sandbox botmode=walk");
        let before = Vec3::from_array(walk.player(3).unwrap().p);
        let mut far = 0f32;
        for _ in 0..12 {
            run(&mut walk, 0.25);
            far = far.max(Vec3::from_array(walk.player(3).unwrap().p).distance(before));
        }
        assert!(far > 0.1 && far < 0.5, "{far}");
    }

    #[test]
    fn free_for_all_is_everyone_against_everyone_with_respawns_spawn_protection_and_the_loadout_of_every_gun_mode() {
        let mut s = siege("mode=ffa");
        assert_eq!((s.stage, s.time_left()), (Stage::FreeForAll, Some(FFA_TIME)));
        assert!(s.players.iter().all(|p| p.ammo[1] == FULL[1] && p.reserve[1] == crate::magazine::SPARE[1] && !s.rules.unlimited()));
        people(&mut s);
        face_off(&mut s);
        s.player_mut(2).unwrap().hp = 10.;
        shoot(&mut s, 1, 2, 2);
        run(&mut s, 0.4);
        assert!(!s.player(2).unwrap().alive);
        assert_eq!((s.player(1).unwrap().kills, s.player(2).unwrap().deaths), (1, 1));
        run(&mut s, FFA_RESPAWN);
        let back = s.player(2).unwrap().clone();
        assert!(back.alive && back.hp == 100. && back.shield > 0., "back with its protection");
        // Protected: a hit takes nothing, until it fires.
        let (o, t) = (crate::feast::muzzle(s.player(1).unwrap()), Vec3::from_array(back.p));
        s.harm(2, 30., true, Damage::by(1, 1, o, t - o));
        assert_eq!(s.player(2).unwrap().hp, 100.);
        s.player_mut(2).unwrap().cooldown = 0.2;
        run(&mut s, 0.05);
        assert_eq!(s.player(2).unwrap().shield, 0.);
        s.harm(2, 30., true, Damage::by(1, 1, o, t - o));
        assert_eq!(s.player(2).unwrap().hp, 70.);
        // No wave, no hunger, nobody's round ends with one left standing.
        run(&mut s, 30.);
        assert!(s.enemies.is_empty() && s.stage == Stage::FreeForAll);
    }

    #[test]
    fn free_for_all_comes_back_far_from_the_living_and_is_won_by_kills_or_the_clock() {
        let mut s = siege("mode=ffa");
        people(&mut s);
        let at = s.level.start(0).0;
        put(&mut s, 2, at.x, at.z + 0.05);
        s.hurt(3, 500.);
        run(&mut s, FFA_RESPAWN + 0.05);
        let back = Vec3::from_array(s.player(3).unwrap().p);
        let nearest = s.players.iter().filter(|p| p.id != 3 && p.alive).map(|p| Vec3::from_array(p.p).distance(back)).fold(f32::INFINITY, f32::min);
        assert!(nearest >= crate::spawns::CLOSE, "{back} {nearest}");
        // FFA_KILLS kills win it.
        s.player_mut(4).unwrap().kills = FFA_KILLS;
        run(&mut s, 0.05);
        assert_eq!((s.stage, s.winner), (Stage::RoundOver, Some(4)));
        assert!(s.alert.contains(&format!("WINS WITH {FFA_KILLS} KILLS")), "{}", s.alert);
        // Or the clock: the most kills; a tie is a draw.
        let mut s = siege("mode=ffa");
        people(&mut s);
        s.player_mut(2).unwrap().kills = 3;
        s.ends = s.elapsed + 0.1;
        run(&mut s, 0.2);
        assert_eq!((s.stage, s.winner), (Stage::RoundOver, Some(2)));
        let mut s = siege("mode=ffa");
        people(&mut s);
        s.ends = s.elapsed + 0.1;
        run(&mut s, 0.2);
        assert_eq!((s.stage, s.winner), (Stage::RoundOver, None));
        // The next round is Free For All again, its clock afresh.
        s.reset();
        assert_eq!((s.stage, s.time_left()), (Stage::FreeForAll, Some(FFA_TIME)));
        assert!(s.players.iter().all(|p| p.kills == 0 && p.deaths == 0));
    }

    #[test]
    fn free_for_all_on_a_forts_map_leaves_the_forcefield_and_teams_out() {
        let mut s = forts("mode=ffa");
        assert!(s.fort.is_none() && s.fort_state().is_none());
        assert_eq!((s.players.len(), s.stage), (NAMES.len(), Stage::FreeForAll));
        assert!(!s.allied(1, 3), "no team mates");
        assert!(s.pickups.iter().all(|p| p.ready <= s.elapsed), "the guns are out");
        run(&mut s, 2.);
        s.reset();
        assert!(s.fort.is_none() && s.players.len() == NAMES.len());
    }
}

#[cfg(test)]
#[path = "modes_tests.rs"]
mod modes_tests;

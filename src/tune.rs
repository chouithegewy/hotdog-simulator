// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The sandbox's tuning panel's side in the game (the page's tune.js, which index.html loads with `?sandbox`). The
//! game publishes what the panel shows on `window.hotdogTune` (whether this is a sandbox or Free For All, whether the
//! tuning may be changed here, every value with its range and default: kitchen_sim::tune's `knobs`, the values now,
//! and what differs from the defaults as text, which the panel's Copy copies) and fires `hotdog:tune` when any of it
//! changes. The panel puts its changes on `window.hotdogTuneQueue` (`mustard.speed=1.1`, `reset sniper`, `reset`),
//! which this drains every frame: single player applies them to its own round at once (in the sandbox), online they
//! go to the server (network.rs), which takes them in a sandbox on a --dev server and tells every player the tuning.
use bevy::prelude::*;

/// What the panel was last told about, compared every frame (cheaply: the tuning's text is only written when it
/// changes).
type Shown = (kitchen_sim::tune::Tuning, bool, bool, bool, bool, &'static str);

/// Every frame, after the arena's tick: the panel's changes taken, and the panel told when what it shows changes.
pub fn bridge(world: &mut World, mut shown: Local<Option<Shown>>) {
    let asked = take();
    if !asked.is_empty() {
        world.resource_scope(|world, mut g: Mut<crate::arena::Game>| {
            for text in asked {
                if g.network {
                    if g.tune_editable {
                        world.resource_mut::<crate::network::Network>().tunes.push(text);
                    }
                } else if g.feast.rules.sandbox {
                    g.tuning.apply(&text);
                    g.feast.tune = g.tuning.clone();
                }
            }
        });
    }
    let g = world.resource::<crate::arena::Game>();
    let rules = &g.feast.rules;
    let editable = if g.network { g.tune_editable } else { rules.sandbox };
    let weapon = kitchen_sim::rules::WEAPONS.get(g.weapon as usize).copied().unwrap_or("mustard");
    if shown.as_ref().is_some_and(|s| s.0 == g.tuning && s.1 == editable && s.2 == rules.sandbox && s.3 == rules.ffa && s.4 == g.network && s.5 == weapon) {
        return;
    }
    *shown = Some((g.tuning.clone(), editable, rules.sandbox, rules.ffa, g.network, weapon));
    let groups: Vec<serde_json::Value> = kitchen_sim::tune::GROUPS.iter().map(|(id, title)| serde_json::json!({"id": id, "title": title})).collect();
    let state = serde_json::json!({
        "sandbox": rules.sandbox,
        "ffa": rules.ffa,
        "editable": editable,
        "online": g.network,
        "weapon": if weapon == "melee" { "mustard" } else { weapon },
        "groups": groups,
        "schema": kitchen_sim::tune::Tuning::knobs(),
        "values": g.tuning.values(),
        "text": g.tuning.text(),
    });
    publish(&state.to_string());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function tune_take(){const q=window.hotdogTuneQueue;if(!Array.isArray(q)||!q.length)return '';return q.splice(0).filter(s=>typeof s==='string'&&s.length<4000).join('\\n')}
export function tune_publish(state){try{window.hotdogTune=JSON.parse(state);window.dispatchEvent(new Event('hotdog:tune'))}catch(e){console.error(e)}}
")]
extern "C" {
    fn tune_take() -> String;
    fn tune_publish(state: &str);
}
/// What the panel has asked for since the last frame, a change (or reset) each.
fn take() -> Vec<String> {
    #[cfg(target_arch = "wasm32")]
    {
        return tune_take().split('\n').filter(|s| !s.trim().is_empty()).map(str::to_string).collect();
    }
    #[cfg(not(target_arch = "wasm32"))]
    vec![]
}
fn publish(state: &str) {
    #[cfg(target_arch = "wasm32")]
    tune_publish(state);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
}

#[cfg(test)]
mod tests {
    use crate::runtime::Options;
    use serde_json::json;
    #[test]
    fn the_address_asks_for_the_sandbox_in_the_join_and_single_player_plays_by_it() {
        let options = Options { params: "mp sandbox weapons=mustard,sniper ammo=inf bots=2 botmode=idle mode=forts map=jungle-fort check".into(), ..Default::default() };
        assert_eq!(options.sandbox(), Some(json!({"sandbox": "", "weapons": "mustard,sniper", "ammo": "inf", "bots": "2", "botmode": "idle", "mode": "forts", "map": "jungle-fort"})));
        let rules = options.rules();
        assert!(rules.sandbox && rules.ammo && rules.weapons == vec![1, 2] && rules.bots == Some(2));
        // Free For All alone is the lobby's to choose online: no sandbox in the join, but single player plays it.
        let ffa = Options { params: "play mode=ffa".into(), ..Default::default() };
        assert_eq!(ffa.sandbox(), None);
        assert!(ffa.rules().ffa && !ffa.rules().sandbox);
        assert_eq!(Options::default().sandbox(), None);
    }
    #[test]
    fn y_goes_round_every_gun_held_in_slot_order() {
        use crate::arena::held_guns;
        let mut owned = [false; kitchen_sim::feast::GUNS];
        assert!(held_guns(&owned).is_empty());
        for w in [1, 2, 4, 6] {
            owned[w] = true;
        }
        let names: Vec<&str> = held_guns(&owned).iter().map(|w| w.name()).collect();
        assert_eq!(names, ["SYRUP RIFLE", "KETCHUP SNIPER", "RELISH BAZOOKA", "MUSTARD PISTOL"]);
    }
}

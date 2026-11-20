// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The protocol: what clients send and are sent over WebSocket (protocol 12).
//!
//! Game socket (`/ws`; the game, in a browser or native):
//!   in   text {type:'join',version:12,token?|name?,color?,team?,json?,sandbox?}  {type:'ready',ready}
//!        binary: batches of commands with the client's ack (kitchen_sim::net `Batch`)
//!   out  text {type:'welcome',id,version,map,tick,send}  {type:'map',id,map}  {type:'error',message}
//!        binary: frames, each against a frame the client acknowledged (kitchen_sim::net `encode_frame`)
//! A socket whose join says `json: true` (or every socket, with the server's --json) is sent its frames as JSON text
//! instead, whole every time ({type:'snapshot',tick,ack,mine?,...}: net.rs `Frame`), and may send its commands as JSON
//! (`{type:'input',commands:[{seq,stick:[x,z],yaw,pitch,crouch,prone,fire,jump,stim,melee,eat,grab,throw,weapon,
//! pickup}]}`): the debug encoding, for the checks.
//!        {type:'tune',text}: the dev sandbox's tuning (kitchen_sim::tune), on a --dev server only
//!   out  {type:'tuning',text,editable}: the tuning (what differs from the constants), after the welcome when there is
//!        any or it may be changed, and to every game socket whenever it changes
//!   Fully Loaded, the card table (kitchen_sim::cards; hub.rs `cards` and `tell_cards`):
//!   in   {type:'cards',action:'flip'|'stop'|'target',seat?}: a flip, a stop, or (`target`, with `seat` a whole number
//!        below the number of seats) the hotdog an action card this player drew is played on. The seat that acts is
//!        always the sender's own hotdog's, never one the message names; what is not that seat's go (or pick) is
//!        refused and anything malformed passed over, both without a word.
//!   out  {type:'cards',table}: the table as everyone sees it (`View` as serde writes it: `seats` [{cards, status,
//!        points, total, napkin, bot}], `dealer`, `hand`, `turn`, `choosing` [seat, card], `clock` (the seconds left of
//!        a person's decision, as it was when sent), `deck`, `between`, `winner`, the latest `events` [{seq, seat, kind,
//!        card, target}] and `version`), to every game socket when it sits down while there is a table and whenever
//!        `version` moves. The frames are unchanged: what they say of a Fully Loaded game is its `rules` (`mode=cards`).
//!   The electric griddle (kitchen_sim::griddle; hub.rs `use_griddle` and `tell_griddles`), on a map that has one:
//!   in   {type:'griddle'}: the sender's hotdog presses use. The griddle flips if the hotdog is within reach of its dial, facing
//!        it, and the server's round is on; whose hotdog it is is the socket's, never the message's. Anything else is passed over.
//!   out  {type:'griddle',griddles:[{block,on,temp,by}]}: each griddle's block in the map, whether it is on, its plate's
//!        temperature in degrees Celsius and the hotdog that last switched it on (0: none), to every game socket when it sits
//!        down, whenever one is flipped (or a new round begins), and every `griddle::SYNC` seconds; a client carries the
//!        temperature on between them by the same law (`Griddle::advance`). The frames are unchanged. A map without one sends nothing.
//! A join's `sandbox` (an object of the page's address as keys and values: kitchen_sim::rules) asks for the dev sandbox;
//! a server started with --dev sets it up when nobody is playing (lobby.rs `sandbox`), any other passes it over. A
//! frame says what the game is played by (`rules`), so a client sees whether it got it.
//! Lobby socket (`/lobby`; a page's lobby.js): see lobby.rs.
//! Beneath all of it, every socket is sent WebSocket ping frames while it is open (transport.rs, --ping): control frames,
//! not messages, which a browser or any WebSocket library answers by itself and which no client reads as a message.
//!
//! Decoding is lenient where the Node server was (a field of the wrong type is ignored) and strict where it was (a
//! socket's first message must be its join or hello).
use glam::Vec2;
use kitchen_sim::net::{Command, button};
use serde::Serialize;
use serde_json::{Value, json};

/// The join handshake's version: clients must be built from the same revision (src/network.rs).
pub const PROTOCOL: u32 = kitchen_sim::net::PROTOCOL;

/// A value as JavaScript's `if (value)` sees it.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0. && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}
/// A finite number (JavaScript's `Number.isFinite`), or None.
pub fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}
/// A whole number (`Number.isInteger`), or None.
pub fn integer(value: &Value) -> Option<f64> {
    number(value).filter(|n| n.fract() == 0.)
}
/// [x, y, z]: an array of three finite numbers, or None.
pub fn vector(value: &Value) -> Option<[f64; 3]> {
    let list = value.as_array().filter(|list| list.len() == 3)?;
    Some([number(&list[0])?, number(&list[1])?, number(&list[2])?])
}
/// What `Number(value)` makes of a value that should be a number, 0 for anything it cannot read (NaN).
pub fn numeric(value: &Value) -> f64 {
    let n = match value {
        Value::Number(n) => n.as_f64().unwrap_or(0.),
        Value::Bool(b) => *b as u8 as f64,
        Value::String(s) => {
            let s = s.trim_matches(crate::text::js_space);
            if s.is_empty() { 0. } else { s.parse().unwrap_or(f64::NAN) }
        }
        _ => 0.,
    };
    if n.is_nan() { 0. } else { n }
}
/// Whether a message's `type` is `kind`.
pub fn is(message: &Value, kind: &str) -> bool {
    message.get("type").and_then(Value::as_str) == Some(kind)
}

/// Serialises a message.
pub fn encode(message: &impl Serialize) -> String {
    serde_json::to_string(message).unwrap_or_else(|_| "{}".into())
}
pub fn error(message: &str) -> String {
    encode(&json!({"type": "error", "message": message}))
}
/// The welcome on a game socket: the hotdog it plays, the map's canonical text, and the steps and frames a second.
pub fn welcome(id: u32, map: &str, tick: u32, send: u32) -> String {
    encode(&json!({"type": "welcome", "id": id, "version": PROTOCOL, "map": map, "tick": tick, "send": send}))
}
/// The commands of a JSON `input` message (the debug encoding), as a binary batch would have them: a field that is
/// missing or not of its type counts as not pressed (or 0). None when there are none.
pub fn commands(message: &Value) -> Vec<Command> {
    let Some(list) = message.get("commands").and_then(Value::as_array) else {
        return vec![];
    };
    list.iter()
        .take(kitchen_sim::net::MAX_COMMANDS)
        .filter_map(|c| {
            let seq = integer(&c["seq"]).filter(|n| *n >= 1. && *n <= u32::MAX as f64)? as u32;
            let stick = c["stick"].as_array().filter(|s| s.len() == 2).map_or(Vec2::ZERO, |s| Vec2::new(number(&s[0]).unwrap_or(0.) as f32, number(&s[1]).unwrap_or(0.) as f32));
            let mut buttons = 0;
            for (key, bit) in [("crouch", button::CROUCH), ("prone", button::PRONE), ("fire", button::FIRE), ("jump", button::JUMP), ("stim", button::STIM), ("melee", button::MELEE), ("eat", button::EAT), ("grab", button::GRAB), ("throw", button::THROW), ("scope", button::SCOPE)] {
                if truthy(&c[key]) {
                    buttons |= bit;
                }
            }
            // How a carried object is held: its turn (eighths of a turn) and its tip.
            let pose = kitchen_sim::feast::build::Pose { turn: integer(&c["turn"]).filter(|t| (0. ..8.).contains(t)).unwrap_or(0.) as u8, tip: integer(&c["tip"]).filter(|t| (0. ..3.).contains(t)).unwrap_or(0.) as u8 };
            buttons |= Command::posed(pose);
            // R: a reload, both of TIP's bits (`button::RELOAD`), with no pose beside it.
            if truthy(&c["reload"]) {
                buttons = buttons & !(button::TURN | button::TIP) | button::RELOAD;
            }
            let weapon = integer(&c["weapon"]).filter(|w| (0. ..=6.).contains(w)).unwrap_or(0.) as u8;
            let pickup = integer(&c["pickup"]).filter(|p| *p >= 0. && *p <= u16::MAX as f64).map(|p| p as usize);
            Some(Command::new(seq, stick, number(&c["yaw"]).unwrap_or(0.) as f32, number(&c["pitch"]).unwrap_or(0.) as f32, buttons, weapon, pickup))
        })
        .collect()
}
/// A round starts on another map: the game socket's hotdog there and the map's canonical text.
pub fn map(id: u32, map: &str) -> String {
    encode(&json!({"type": "map", "id": id, "map": map}))
}
pub const MISMATCH: &str = "Protocol version 12 required: rebuild the game client";
pub const FULL: &str = "Kitchen full (5 hotdogs)";
pub const UNKNOWN_PAGE: &str = "The lobby does not know this page: reload it";
pub const NO_PLACE: &str = "There was no place on the new map: reload the page";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn values_are_read_as_javascript_reads_them() {
        for (value, expected) in [(json!(null), false), (json!(0), false), (json!(1), true), (json!(""), false), (json!("x"), true), (json!([]), true), (json!({}), true), (json!(false), false)] {
            assert_eq!(truthy(&value), expected, "{value}");
        }
        assert_eq!(vector(&json!([1, 2.5, -3])), Some([1., 2.5, -3.]));
        assert_eq!(vector(&json!([1, "2", 3])), None);
        assert_eq!(vector(&json!([1, 2])), None);
        assert_eq!(integer(&json!(2.0)), Some(2.));
        assert_eq!(integer(&json!(2.5)), None);
        assert_eq!(numeric(&json!("0.5")), 0.5);
        assert_eq!(numeric(&json!("loud")), 0.);
        assert_eq!(numeric(&json!(true)), 1.);
        assert_eq!(MISMATCH, format!("Protocol version {PROTOCOL} required: rebuild the game client"));
    }
    #[test]
    fn json_commands_read_as_binary_ones_do() {
        let c = commands(&json!({"type": "input", "commands": [{"seq": 4, "stick": [0, -1], "yaw": 1.5, "pitch": -0.2, "fire": true, "jump": 1, "weapon": 2, "pickup": 3}, {"seq": "5"}, {"seq": 6, "crouch": true, "weapon": 9}]}));
        assert_eq!(c.len(), 2, "a command without a number is passed over");
        assert_eq!(c[0], Command::new(4, Vec2::new(0., -1.), 1.5, -0.2, button::FIRE | button::JUMP, 2, Some(3)));
        assert_eq!((c[1].seq, c[1].buttons, c[1].weapon), (6, button::CROUCH, 0));
        assert!(commands(&json!({"type": "input", "fire": true})).is_empty());
    }
}

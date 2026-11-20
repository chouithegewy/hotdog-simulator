// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The viewer's side of the page (index.html, touch.js): what it tells the page of the replay, and what the page's
//! controls ask of it.
//!
//! * **What it tells** (`window.hotdogReplay`, and a `hotdog:replay` event on the window when it changes, a few times
//!   a second at most): `loading`; `playing`, with what touch.js's controls show (the clock and the length, paused or
//!   not, the speed, the view and whose it is, the stats, the timeline's markers); or `failed`, with why and of what
//!   kind (`missing`, `network`, `server`, `version`, `damaged`), so that the page says why a replay cannot be watched
//!   instead of leaving the screen blank, and a check can tell that one plays.
//! * **What the page asks**, on a touch screen (touch.js shows the viewer's own controls there instead of the game's,
//!   and the bar at the bottom of ui.rs gives way to them): words queued in `window.hotdogTouch.asks` (`play`, `back`,
//!   `ahead`, `speed:N`, `seek:F`, a share of the timeline, `view`, `next`, `previous`, `stats`, `next-kill`,
//!   `previous-kill`), read once a frame. A drag looks round, two fingers fly the free camera and pinch its speed or the
//!   follow camera's distance (touch.rs `Pad`, read by mod.rs `fly`).
use super::{Mode, SPEEDS, Viewer};
use bevy::prelude::*;

/// What a control on the page asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
    Play,
    Back,
    Ahead,
    Speed(usize),
    /// A share of the timeline (0 to 1).
    Seek(f64),
    View,
    Next,
    Previous,
    Stats,
    NextKill,
    PreviousKill,
}
/// A word of the page's as what it asks (None for one this build does not know, or out of range).
pub fn parse(word: &str) -> Option<Ask> {
    let (head, arg) = word.trim().split_once(':').unwrap_or((word.trim(), ""));
    Some(match head {
        "play" => Ask::Play,
        "back" => Ask::Back,
        "ahead" => Ask::Ahead,
        "speed" => Ask::Speed(arg.parse::<usize>().ok().filter(|i| *i < SPEEDS.len())?),
        "seek" => Ask::Seek(arg.parse::<f64>().ok().filter(|f| f.is_finite())?.clamp(0., 1.)),
        "view" => Ask::View,
        "next" => Ask::Next,
        "previous" => Ask::Previous,
        "stats" => Ask::Stats,
        "next-kill" => Ask::NextKill,
        "previous-kill" => Ask::PreviousKill,
        _ => return None,
    })
}
/// Does what the page asked (as the viewer's keys and its bar do).
pub fn apply(world: &mut World, v: &mut Viewer, ask: Ask) {
    match ask {
        Ask::Play => super::toggle_play(world, v),
        Ask::Back => v.seek = Some((v.clock - super::JUMP as f64).max(0.)),
        Ask::Ahead => v.seek = Some((v.clock + super::JUMP as f64).min(v.duration())),
        Ask::Speed(i) => v.speed = i,
        Ask::Seek(share) => v.seek = Some(share * v.duration()),
        Ask::View => {
            let next = v.mode.next();
            super::set_mode(world, v, next);
        }
        Ask::Next => v.step_player(1),
        Ask::Previous => v.step_player(-1),
        Ask::Stats => v.show.stats = !v.show.stats,
        Ask::NextKill | Ask::PreviousKill => {
            if let Some(at) = v.kill(if ask == Ask::NextKill { 1 } else { -1 }) {
                v.seek = Some((at - 1500.).max(0.));
            }
        }
    }
}
/// Every frame the viewer's keys are heard: what the page asked since the last.
pub fn asks(world: &mut World, v: &mut Viewer) {
    for word in web::asks().lines() {
        if let Some(ask) = parse(word) {
            apply(world, v, ask);
        }
    }
}
/// Whether the page's touch controls have the viewer's bar (ui.rs then hides its own).
pub fn controls() -> bool {
    crate::touch::on()
}

/// Why a replay's file could not be had (store.rs `poll`'s error: the page's fetch, or this browser's storage), and of
/// what kind: what the page says of it.
pub fn failure(error: &str) -> (&'static str, String) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("http 404") || lower.contains("has no replay") {
        return ("missing", "This replay is not on the server: it may have been replaced by newer ones (the server keeps only its latest matches), or the link is mistyped.".into());
    }
    if let Some(status) = lower.split("http ").nth(1).and_then(|s| s.split_whitespace().next()) {
        return ("server", format!("The server could not send this replay (HTTP {status}). Try again in a moment."));
    }
    if lower.contains("could not be read") {
        return ("missing", error.into());
    }
    ("network", format!("The replay could not be downloaded ({error}). Check the connection and try again."))
}
/// Why a file that came cannot be played, and of what kind.
pub fn unplayable(error: &kitchen_sim::replay::Error) -> (&'static str, String) {
    use kitchen_sim::replay::Error;
    (if matches!(error, Error::Format { .. } | Error::NotAReplay) { "version" } else { "damaged" }, error.to_string())
}

/// What the page is told (see the module's documentation): the clock in tenths of a second, so that it changes a few
/// times a second at most.
pub fn state(v: &Viewer) -> serde_json::Value {
    if let Some(why) = &v.failed {
        let (kind, message) = v.failure.clone().unwrap_or_else(|| ("damaged", why.clone()));
        return serde_json::json!({"state": "failed", "source": v.source, "kind": kind, "message": message});
    }
    let Some(replay) = v.replay.as_ref() else {
        return serde_json::json!({"state": "loading", "source": v.source});
    };
    let duration = v.duration();
    let marks: Vec<(u32, u8)> = replay.markers().map(|m| (m.ms, m.kind as u8)).collect();
    serde_json::json!({
        "state": "playing",
        "source": v.source,
        "title": super::ui::title(v),
        "warning": v.warning,
        "clock": (v.clock / 100.).round() * 100.,
        "duration": duration,
        "paused": v.paused,
        "ended": v.ended,
        "speed": v.speed,
        "speeds": (0..SPEEDS.len()).map(super::ui::speed_name).collect::<Vec<_>>(),
        "view": v.mode.name(),
        "mode": match v.mode { Mode::First => "first", Mode::Follow => "follow", Mode::Fly => "fly" },
        "who": super::ui::watching(v),
        "players": v.players().len(),
        "recorded": v.recorded(),
        "stats": v.show.stats,
        "marks": marks,
    })
}
/// Tells the page when what it is told has changed (and the clock at most ten times a second).
pub fn tell(v: &mut Viewer, now: f64) {
    let text = state(v).to_string();
    if text == v.told || now - v.told_at < 0.1 && v.failed.is_none() {
        return;
    }
    web::tell(&text);
    (v.told, v.told_at) = (text, now);
}

#[cfg(target_arch = "wasm32")]
mod web {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function replay_tell(text){let v;try{v=JSON.parse(text)}catch{return}window.hotdogReplay=v;const t=window.hotdogTouch;if(t&&typeof t.replay==='function'){try{t.replay(v)}catch(e){console.error(e)}}window.dispatchEvent(new CustomEvent('hotdog:replay',{detail:v}))}
export function replay_asks(){const t=window.hotdogTouch;if(!t||!Array.isArray(t.asks)||!t.asks.length)return '';return t.asks.splice(0,32).join('\\n')}
")]
    extern "C" {
        fn replay_tell(text: &str);
        fn replay_asks() -> String;
    }
    pub fn tell(text: &str) {
        replay_tell(text);
    }
    pub fn asks() -> String {
        replay_asks()
    }
}
#[cfg(not(target_arch = "wasm32"))]
mod web {
    pub fn tell(_: &str) {}
    pub fn asks() -> String {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_pages_words_are_what_the_controls_ask_and_nothing_else() {
        assert_eq!(parse("play"), Some(Ask::Play));
        assert_eq!(parse(" ahead "), Some(Ask::Ahead));
        assert_eq!(parse("speed:3"), Some(Ask::Speed(3)));
        assert_eq!(parse("speed:4"), None, "only the speeds there are");
        assert_eq!(parse("speed:x"), None);
        assert_eq!(parse("seek:0.25"), Some(Ask::Seek(0.25)));
        assert_eq!(parse("seek:7"), Some(Ask::Seek(1.)), "a share of the timeline");
        assert_eq!(parse("seek:NaN"), None);
        assert_eq!(parse("previous-kill"), Some(Ask::PreviousKill));
        assert_eq!(parse("view"), Some(Ask::View));
        assert_eq!(parse("rm -rf"), None);
        assert_eq!(parse(""), None);
    }
    #[test]
    fn a_failure_says_what_kind_it_is() {
        assert_eq!(failure("replays/abc: HTTP 404").0, "missing");
        assert_eq!(failure("This browser has no replay x").0, "missing");
        assert_eq!(failure("replays/abc: HTTP 503").0, "server");
        assert!(failure("replays/abc: HTTP 503").1.contains("503"));
        assert_eq!(failure("Load failed").0, "network", "Safari's word for a fetch that failed");
        assert_eq!(failure("Failed to fetch").0, "network");
        assert!(failure("Load failed").1.contains("Check the connection"));
        assert_eq!(failure("/tmp/x.replay could not be read: No such file").0, "missing");
        use kitchen_sim::replay::Error;
        let format = Error::Format { found: 9 };
        assert_eq!(unplayable(&format), ("version", format.to_string()));
        assert_eq!(unplayable(&Error::NotAReplay).0, "version");
        assert_eq!(unplayable(&Error::Damaged("its header does not read (format)".into())).0, "damaged");
    }
}

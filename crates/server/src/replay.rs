// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Replays (kitchen_sim::replay): every match the server plays is recorded while anybody is playing it, from a
//! spectator's view, into its replays folder (`--replays DIR`, `replays/` in the served root by default), and the page
//! can list them (`GET /replays`) and fetch one (`GET /replays/<id>`).
//!
//! **What is recorded.** Each game socket is sent its own frame: the shared state of the game encoded against what that
//! client acknowledged, with its own ack and body. Those differ only in their encoding and in the client's own body
//! (for its prediction), so the recording is one neutral stream of the shared state, each frame encoded against the one
//! before, with every text the game sockets are all sent beside the frames (the card table, the griddles, Kung Fu, the
//! reserves, the tuning). Whatever a spectator needs to see any player's view is in it: every hotdog's place, aim, health
//! and weapon is in the shared state.
//!
//! **A match** is recorded from the fridge opening (or the first frame of a game with no lobby: the demo, the sandbox)
//! until the game goes back to the lobby, another map's game takes over, or the last game socket leaves; a recording
//! longer than MAX_MS is cut into parts. Nothing is recorded while nobody plays (a demo server nobody watches). A
//! recording is written as it goes, a chunk every few seconds, so a server that dies leaves a file that plays up to
//! there. The folder keeps the newest `--replays-keep` recordings (50); older ones are deleted, oldest first.
//!
//! **Listing** (`GET /replays`): `{"replays":[{id, date, started, mode, map, title, players, duration, winner, size,
//! complete, views}]}`, newest first: the names are those the game already shows over the hotdogs, nothing else of
//! anybody (`views`: the ids of the players whose own view it holds).
//!
//! **Players' own views** (kitchen_sim::pov): a client may send what its own screen showed, in batches of about a
//! second (`{"type":"pov","header":{...}}` once, then `{"type":"pov","seq":n,"data":"<base64>"}`), which are kept in
//! the recording beside the neutral stream, each player's on its own (`pov`). The server never plays them and nothing
//! of the match depends on them: each batch is checked (kitchen_sim::pov `check`: its size, that it reads whole, its
//! clock forward and within the match, its frames within the rate) and the sender held to at most RATE batches a second
//! and CAP_PER_MINUTE bytes a minute of recording (CAP_MOST in all). A player's track that breaks a rule is dropped for
//! the rest of the recording (a note of why is kept in the file); the player plays on as before.
use crate::lobby::Lobby;
use kitchen_sim::net::{Frame, State};
use kitchen_sim::pov;
use kitchen_sim::replay::{self, Header, Writer};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// The most recordings kept by default.
pub const KEEP: usize = 50;
/// A recording longer than this (milliseconds) is cut into parts.
pub const MAX_MS: u32 = 2 * 60 * 60 * 1000;
/// A player's own view: the most batches a second it may send (and the most at once after a quiet spell), the bytes it
/// may keep a minute of recording (beyond a first allowance), and in all.
pub const RATE: f64 = 4.;
pub const BURST: f64 = 8.;
pub const CAP_PER_MINUTE: u64 = 300 * 1024;
pub const CAP_FIRST: u64 = 512 * 1024;
pub const CAP_MOST: u64 = 32 << 20;
/// How far before the recording's start and after the server's clock a batch's frames may be (ms): a client draws a
/// little in the past, and its clock is the server's steps as it saw them.
const EARLY_MS: f64 = 5000.;
const LATE_MS: f64 = 2000.;
/// What wrote a server's recordings.
pub fn build() -> String {
    format!("hotdog-server {}", env!("CARGO_PKG_VERSION"))
}

/// A recording being written.
struct Recording {
    writer: Writer<BufWriter<File>>,
    path: PathBuf,
    game: u64,
    start: u32,
    hz: u32,
    /// Each player's own view in it: whether its header is written, the bytes kept, whether it was dropped.
    views: BTreeMap<u32, View>,
}
#[derive(Default)]
struct View {
    begun: bool,
    bytes: u64,
    dropped: bool,
}
/// What a player's client has said of its own view: its header (JSON, checked), and the batches it may send now (a
/// bucket refilled at RATE a second), as of the server's clock (ms).
#[derive(Clone)]
struct Sender {
    header: Option<String>,
    tokens: f64,
    at: f64,
}
/// The server's recorder (see the module's documentation). Off without a folder.
pub struct Replays {
    pub dir: Option<PathBuf>,
    pub keep: usize,
    current: Option<Recording>,
    /// The latest text of each kind every game socket was sent, and the game it was of: the first chunk's context.
    latest: BTreeMap<String, String>,
    latest_game: u64,
    /// Whether the folder could not be written (said once).
    failed: bool,
    /// Each player's client's own view, as it said it.
    senders: BTreeMap<u32, Sender>,
    /// What became of the batches sent (for the checks and the status): kept, refused.
    pub views_kept: u64,
    pub views_refused: u64,
}
impl Default for Replays {
    fn default() -> Replays {
        Replays::off()
    }
}
impl Replays {
    pub fn off() -> Replays {
        Replays { dir: None, keep: KEEP, current: None, latest: BTreeMap::new(), latest_game: 0, failed: false, senders: BTreeMap::new(), views_kept: 0, views_refused: 0 }
    }
    /// Recording into `dir`, keeping the newest `keep` (0: recording nothing).
    pub fn new(dir: PathBuf, keep: usize) -> Replays {
        let mut r = Replays::off();
        (r.dir, r.keep) = ((keep > 0).then_some(dir), keep);
        r.prune();
        r
    }
    /// Whether a recording is being written, and its id.
    pub fn recording(&self) -> Option<String> {
        self.current.as_ref().and_then(|r| r.path.file_stem()).map(|s| s.to_string_lossy().into())
    }
    /// A text every game socket of game `game` was sent at step `tick`.
    pub fn text(&mut self, game: u64, tick: u32, text: &str) {
        if self.dir.is_none() {
            return;
        }
        if game != self.latest_game {
            (self.latest_game, self.latest) = (game, BTreeMap::new());
        }
        if let Some(kind) = replay::kind_of(text).filter(|k| replay::CONTEXT.contains(&k.as_str())) {
            self.latest.insert(kind, text.to_string());
        }
        if let Some(r) = self.current.as_mut().filter(|r| r.game == game) {
            let ms = ms(tick, r.start, r.hz);
            r.writer.text(ms, text);
        }
    }
    /// A round of frames at step `tick` of game `game` (steps `hz` a second), `players` game sockets playing: begins,
    /// goes on with or ends the recording (see the module's documentation).
    #[allow(clippy::too_many_arguments)]
    pub fn frame(&mut self, lobby: &Lobby, game: u64, tick: u32, hz: u32, players: usize, frame: &Frame, state: &State) {
        if self.dir.is_none() {
            return;
        }
        let playing = players > 0 && frame.stage != "Lobby";
        let stale = self.current.as_ref().is_some_and(|r| r.game != game || ms(tick, r.start, hz) > MAX_MS);
        if (!playing || stale) && self.current.is_some() {
            self.finish();
        }
        if !playing {
            return;
        }
        if game != self.latest_game {
            (self.latest_game, self.latest) = (game, BTreeMap::new());
        }
        if self.current.is_none() {
            self.begin(lobby, game, tick, hz, frame);
        }
        let Some(r) = self.current.as_mut() else { return };
        if let Err(error) = r.writer.frame(ms(tick, r.start, hz), tick, frame, state) {
            eprintln!("Replay {} could not be written ({error}): recording stopped", r.path.display());
            self.current = None;
        }
    }
    /// Begins a recording of the game being played.
    fn begin(&mut self, lobby: &Lobby, game: u64, tick: u32, hz: u32, frame: &Frame) {
        let Some(dir) = self.dir.clone() else { return };
        let g = &lobby.game;
        // (The mode the game is played in, the rules' or else the map's own: a Forts map may be played as a siege.)
        let mode = g.feast.mode().name();
        let started = now();
        let mut header = Header::new(&build(), "server", &g.map, mode, &g.rules().text(), replay::roster(frame), started);
        header.tick_hz = hz;
        header.start_ms = tick as f64 * 1000. / hz.max(1) as f64;
        let id = unique(&dir, &format!("{}-{}", stamp(started), slug(&g.map.name)));
        let path = dir.join(format!("{id}.{}", replay::EXTENSION));
        let opened = std::fs::create_dir_all(&dir).and_then(|_| File::create(&path)).and_then(|file| Writer::new(BufWriter::new(file), &header));
        match opened {
            Ok(mut writer) => {
                // What every socket was told before it began: the first chunk's context.
                let tune = g.feast.tune.text();
                if !tune.is_empty() {
                    writer.text(0, &replay::say::tuning(&tune));
                }
                for text in self.latest.values() {
                    writer.text(0, text);
                }
                println!("Recording a replay: {}", path.display());
                self.failed = false;
                self.current = Some(Recording { writer, path, game, start: tick, hz, views: BTreeMap::new() });
            }
            Err(error) => {
                if !self.failed {
                    eprintln!("Replays cannot be written to {} ({error}): not recording", dir.display());
                    self.failed = true;
                }
            }
        }
    }
    /// A player's client's own view (see the module's documentation): `{"type":"pov","header":{...}}` or
    /// `{"type":"pov","seq":n,"data":"<base64 batch>"}` from player `player` of game `game`, at step `tick` (`hz` steps
    /// a second). Never more than a note on the server's console comes of one that is wrong. Returns whether it was
    /// kept (a header is kept for the recordings to come).
    #[allow(clippy::too_many_arguments)]
    pub fn pov(&mut self, game: u64, tick: u32, hz: u32, player: u32, message: &serde_json::Value) -> bool {
        if self.dir.is_none() {
            return false;
        }
        let now = tick as f64 * 1000. / hz.max(1) as f64;
        let sender = self.senders.entry(player).or_insert(Sender { header: None, tokens: BURST, at: now });
        // The bucket: RATE a second, BURST at most.
        sender.tokens = (sender.tokens + (now - sender.at).max(0.) / 1000. * RATE).min(BURST);
        sender.at = now;
        let Some(r) = self.current.as_mut().filter(|r| r.game == game) else {
            // (Between recordings a header is still taken: the next recording begins with it.)
            if let Some(h) = message.get("header") {
                sender.header = pov::Header::parse(&h.to_string()).ok().filter(|h| h.player == player).map(|_| h.to_string());
            }
            return false;
        };
        let view = r.views.entry(player).or_default();
        if view.dropped {
            self.views_refused += 1;
            return false;
        }
        let refuse = |r: &mut Recording, why: &str| {
            let view = r.views.entry(player).or_default();
            view.dropped = true;
            eprintln!("Replay {}: player {player}'s own view is no longer kept ({why})", r.path.display());
            let _ = r.writer.pov_drop(player, why);
        };
        if sender.tokens < 1. {
            refuse(r, "batches too fast");
            self.views_refused += 1;
            return false;
        }
        sender.tokens -= 1.;
        if let Some(h) = message.get("header") {
            let text = h.to_string();
            match pov::Header::parse(&text) {
                Ok(header) if header.player == player && header.clock == "server" => {
                    sender.header = Some(text);
                    view.begun = false;
                    return true;
                }
                _ => {
                    refuse(r, "a header that does not read, or is not its own");
                    self.views_refused += 1;
                    return false;
                }
            }
        }
        let data = message.get("data").and_then(|d| d.as_str()).ok_or(kitchen_sim::net::Malformed("a message without data")).and_then(pov::unbase64);
        let start = r.start as f64 * 1000. / r.hz.max(1) as f64;
        let checked = data.and_then(|bytes| pov::check(&bytes, start - EARLY_MS, now + LATE_MS).map(|_| bytes));
        let bytes = match (checked, &sender.header) {
            (Ok(bytes), Some(_)) => bytes,
            (Ok(_), None) => {
                refuse(r, "a batch before its header");
                self.views_refused += 1;
                return false;
            }
            (Err(why), _) => {
                refuse(r, why.0);
                self.views_refused += 1;
                return false;
            }
        };
        let minutes = (now - start).max(0.) / 60_000.;
        let allowed = (CAP_FIRST + (CAP_PER_MINUTE as f64 * minutes) as u64).min(CAP_MOST);
        if view.bytes + bytes.len() as u64 > allowed {
            refuse(r, "more than a player's view may keep");
            self.views_refused += 1;
            return false;
        }
        if !view.begun {
            let header = sender.header.clone().unwrap_or_default();
            if r.writer.pov_header(player, start, &header).is_err() {
                return false;
            }
            view.begun = true;
        }
        if r.writer.pov(player, &bytes).is_err() {
            return false;
        }
        view.bytes += bytes.len() as u64;
        self.views_kept += 1;
        true
    }
    /// A player's client has gone: what it said of its own view is forgotten.
    pub fn forget(&mut self, player: u32) {
        self.senders.remove(&player);
    }
    /// Ends the recording being written (its summary written) and keeps the newest `keep`.
    pub fn finish(&mut self) {
        let Some(r) = self.current.take() else { return };
        match r.writer.finish(now()) {
            Ok((_, summary)) => println!("Replay saved: {} ({} s)", r.path.display(), summary.duration_ms / 1000),
            Err(error) => eprintln!("Replay {} could not be finished ({error}): it plays up to where it was written", r.path.display()),
        }
        self.prune();
    }
    /// Deletes the oldest recordings beyond `keep` (never the one being written).
    fn prune(&mut self) {
        let Some(dir) = &self.dir else { return };
        let writing = self.current.as_ref().map(|r| r.path.clone());
        let mut files = files(dir);
        files.retain(|(path, _)| Some(path) != writing.as_ref());
        let keep = self.keep.saturating_sub(writing.is_some() as usize);
        if files.len() <= keep {
            return;
        }
        // Oldest first: by when each was last written, then by name (which begins with when it was started).
        files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        for (path, _) in &files[..files.len() - keep] {
            if std::fs::remove_file(path).is_ok() {
                println!("Replay deleted (more than {} kept): {}", self.keep, path.display());
            }
        }
    }
}
impl Drop for Replays {
    /// A server that stops finishes what it was recording.
    fn drop(&mut self) {
        self.finish();
    }
}
/// Milliseconds from step `start` to step `tick`, `hz` steps a second.
fn ms(tick: u32, start: u32, hz: u32) -> u32 {
    (tick.wrapping_sub(start) as u64 * 1000 / hz.max(1) as u64) as u32
}
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
/// The replay files in `dir`, each with when it was last written.
fn files(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == replay::EXTENSION) && p.file_stem().is_some_and(|s| id(&s.to_string_lossy())))
        .map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (p, modified)
        })
        .collect()
}
/// Whether `text` may be a replay's id: letters, digits, `-` and `_`, as the files are named.
pub fn id(text: &str) -> bool {
    !text.is_empty() && text.len() <= 96 && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
/// A map's name as part of a file's name.
fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "map".into() } else { s.chars().take(40).collect() }
}
/// `base`, or `base-2`, `base-3`... whichever is not taken in `dir`.
fn unique(dir: &Path, base: &str) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while dir.join(format!("{id}.{}", replay::EXTENSION)).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}
/// The calendar date and time (UTC) of Unix second `secs`.
fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // (Howard Hinnant's days-to-civil.)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}
/// `YYYYMMDD-HHMMSS` (UTC), for a file's name.
fn stamp(secs: u64) -> String {
    let (y, m, d, h, mi, s) = civil(secs);
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}")
}
/// ISO 8601 (UTC), for a listing.
fn iso(secs: u64) -> String {
    let (y, m, d, h, mi, s) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// One recording as the listing has it.
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub date: String,
    pub started: u64,
    pub mode: String,
    pub map: String,
    pub players: Vec<String>,
    /// Seconds.
    pub duration: f64,
    pub winner: Option<String>,
    pub size: u64,
    /// Finished (with its summary), not cut short or still being written.
    pub complete: bool,
    /// The players whose own view it holds (kitchen_sim::pov).
    pub views: Vec<u32>,
}
/// The recordings in `dir`, newest first (those that cannot be read are left out).
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = files(dir)
        .into_iter()
        .filter_map(|(path, _)| {
            let mut file = File::open(&path).ok()?;
            let size = file.metadata().ok()?.len();
            let listing = replay::scan(&mut file).ok()?;
            let h = &listing.header;
            let duration = listing.summary.as_ref().map_or(listing.duration_ms, |s| s.duration_ms);
            Some(Entry {
                id: path.file_stem()?.to_string_lossy().into(),
                date: iso(h.started),
                started: h.started,
                mode: h.mode.clone(),
                map: h.map_name.clone(),
                players: h.roster.iter().map(|m| m.name.clone()).collect(),
                duration: duration as f64 / 1000.,
                winner: listing.summary.as_ref().and_then(|s| s.winner.clone()),
                size,
                complete: !listing.truncated,
                views: listing.povs.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.started.cmp(&a.started).then_with(|| b.id.cmp(&a.id)));
    out
}
/// The file of the recording `id` in `dir`, if there is one.
pub fn path(dir: &Path, id: &str) -> Option<PathBuf> {
    let id = id.strip_suffix(".replay").unwrap_or(id);
    let path = dir.join(format!("{id}.{}", replay::EXTENSION));
    (self::id(id) && path.is_file()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Net;
    use crate::game::{Game, Settings};
    use crate::hub::{Event, Hub};
    use crate::lobby::{Lobby, Options};
    use crate::outbox::Outbox;
    use kitchen_sim::map::Map;
    use kitchen_sim::net::{self, decode_frame};
    use kitchen_sim::pov;
    use kitchen_sim::replay::{Decoder, Playhead, Record, Replay};
    use serde_json::{Value, json};

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    /// A folder of its own beside the test binary (in the build's own directory), emptied first.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::current_exe().unwrap().with_file_name(format!("replays-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
    fn demo_hub(dir: &Path) -> Hub {
        let settings = Settings { demo: true, ..Settings::default() };
        let map = Map::parse(&std::fs::read_to_string(format!("{ROOT}/assets/maps/counter-siege.json")).unwrap()).unwrap();
        let game = Game::new(map, &settings);
        let lobby = Lobby::new(game, settings, Options::new(Path::new(ROOT).join("assets/maps")));
        let mut hub = Hub::new(lobby, Net::default());
        hub.replays = Replays::new(dir.to_path_buf(), KEEP);
        hub
    }

    /// The mode a recording says is the game's, not the map's: a siege on a Forts map is a siege, in its header (and
    /// its listing) and in its rules, which every frame carries too.
    #[test]
    fn a_recording_names_the_mode_the_game_is_played_in_not_the_maps() {
        let dir = folder("modes");
        let settings = Settings { demo: true, ..Settings::default() };
        let map = Map::parse(&std::fs::read_to_string(format!("{ROOT}/assets/maps/pillow-fort.json")).unwrap()).unwrap();
        let game = Game::with_rules(map, &settings, kitchen_sim::rules::Rules::parse("mode=siege"));
        let lobby = Lobby::new(game, settings, Options::new(Path::new(ROOT).join("assets/maps")));
        let mut hub = Hub::new(lobby, Net::default());
        hub.replays = Replays::new(dir.to_path_buf(), KEEP);
        let (outbox, _inbox, _frames) = Outbox::memories();
        hub.event(Event::Open { peer: 1, page: false, outbox });
        hub.event(Event::Message { peer: 1, message: json!({"type": "join", "version": 12, "name": "Nell"}) });
        for _ in 0..120 {
            if hub.advance() {
                hub.send();
            }
        }
        let id = hub.replays.recording().expect("recording");
        hub.event(Event::Closed { peer: 1 });
        while !hub.advance() {}
        hub.send();
        let listed = list(&dir);
        assert_eq!((listed[0].map.as_str(), listed[0].mode.as_str()), ("Countertop Fort", "siege"));
        let replay = Replay::open(&path(&dir, &id).unwrap()).unwrap();
        assert_eq!((replay.header.mode.as_str(), replay.header.rules.as_str(), replay.header.map_name.as_str()), ("siege", "mode=siege", "Countertop Fort"));
        assert!(replay.header.map.contains(r#""mode": "forts""#), "the map it was made for is the map's own");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_server_records_a_bot_match_while_somebody_watches_lists_it_and_serves_it() {
        let dir = folder("bots");
        let mut hub = demo_hub(&dir);
        // Nobody playing: nothing recorded.
        for _ in 0..120 {
            if hub.advance() {
                hub.send();
            }
        }
        assert!(hub.replays.recording().is_none() && list(&dir).is_empty());
        // A game socket joins (and never acknowledges: every frame it is sent is whole) and watches the bots.
        let (outbox, inbox, frames) = Outbox::memories();
        hub.event(Event::Open { peer: 1, page: false, outbox });
        hub.event(Event::Message { peer: 1, message: json!({"type": "join", "version": 12, "name": "Nell"}) });
        let welcome: Value = serde_json::from_str(&inbox.lock().unwrap()[0]).unwrap();
        assert_eq!(welcome["type"], "welcome");
        for _ in 0..(60 * 16) {
            if hub.advance() {
                hub.send();
            }
        }
        let id = hub.replays.recording().expect("recording while somebody plays");
        // The socket leaves: the recording is finished.
        hub.event(Event::Closed { peer: 1 });
        while !hub.advance() {}
        hub.send();
        assert!(hub.replays.recording().is_none());
        let listed = list(&dir);
        assert_eq!(listed.len(), 1);
        let e = &listed[0];
        assert_eq!((e.id.as_str(), e.map.as_str(), e.mode.as_str(), e.complete), (id.as_str(), "Counter Siege", "siege", true));
        // (In the demo the bots play, and the socket watches.)
        assert_eq!(e.players.len(), 5, "{:?}", e.players);
        assert!((15.0..17.0).contains(&e.duration), "{}", e.duration);
        assert!(e.date.ends_with('Z') && e.date.len() == 20);
        // What the socket was sent, frame by frame, is what the replay plays at the same steps.
        let replay = Replay::open(&path(&dir, &id).unwrap()).unwrap();
        assert_eq!((replay.header.source.as_str(), replay.header.build.clone()), ("server", build()));
        let sent: Vec<(u32, net::State)> = frames.lock().unwrap().iter().map(|b| decode_frame(b, None).unwrap()).map(|(h, s, _)| (h.tick, s)).collect();
        let (mut head, mut played) = Playhead::seek(&replay, 0).unwrap();
        played.extend(head.advance(&replay, u32::MAX));
        let mut decoder = Decoder::default();
        let mut compared = 0;
        for p in &played {
            if let Record::Frame(bytes) = &p.record {
                let (h, _) = decoder.frame(bytes).unwrap();
                if let Some((_, s)) = sent.iter().find(|(t, _)| *t == h.tick) {
                    assert!(decoder.state() == Some(s), "the frame of step {} is the socket's", h.tick);
                    compared += 1;
                }
            }
        }
        assert!(compared > 400, "{compared} frames compared");
        // The reserves' text every socket was sent is in it.
        assert!(played.iter().any(|p| matches!(&p.record, Record::Text(t) if t.contains("\"ammo\""))));
        // Listed and served over HTTP.
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            use tower::ServiceExt;
            let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
            let app = crate::transport::App::new(PathBuf::from(ROOT), events);
            let app = crate::transport::App::with_replays(app, Some(dir.clone()));
            let get = |path: String| axum::http::Request::get(path).body(axum::body::Body::empty()).unwrap();
            let response = crate::transport::router(app.clone()).oneshot(get("/replays".into())).await.unwrap();
            assert_eq!(response.status(), 200);
            let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
            let v: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(v["replays"][0]["id"], json!(id));
            assert_eq!(v["replays"][0]["players"].as_array().unwrap().len(), 5);
            let response = crate::transport::router(app.clone()).oneshot(get(format!("/replays/{id}"))).await.unwrap();
            assert_eq!(response.status(), 200);
            let body = axum::body::to_bytes(response.into_body(), 64 << 20).await.unwrap();
            assert_eq!(body.as_ref(), std::fs::read(path(&dir, &id).unwrap()).unwrap().as_slice());
            for bad in ["/replays/..%2Fsecret", "/replays/nope", "/replays/a.b"] {
                let response = crate::transport::router(app.clone()).oneshot(get(bad.into())).await.unwrap();
                assert_eq!(response.status(), 404, "{bad}");
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A player's own view as its client sends it: its header, and a batch of `n` frames at 60 a second ending at `to`
    /// (ms on the server's clock), each with a shot.
    fn header(player: u32) -> Value {
        json!({"type": "pov", "header": serde_json::from_str::<Value>(&pov::Header { player, clock: "server".into(), rate: 60, ..Default::default() }.text()).unwrap()})
    }
    fn batch(seq: u32, to: f64, n: usize) -> Value {
        let samples: Vec<pov::Sample> = (0..n).map(|i| pov::Sample { t: to - (n - 1 - i) as f64 * 1000. / 60., now: 5. + i as f64 / 60., cam: [0.1 * i as f32, 1., 0.], fov: 1.4, ..Default::default() }).collect();
        let events = vec![pov::Timed { at: 0, event: pov::Event::Shot { weapon: 1, yaw: 0., pitch: 0. } }];
        json!({"type": "pov", "seq": seq, "data": pov::base64(&pov::Batch { samples, events }.encode())})
    }
    fn server_ms(hub: &Hub) -> f64 {
        hub.tick as f64 * 1000. / hub.net.tick as f64
    }
    fn joined(hub: &mut Hub, peer: u64, name: &str, outbox: Outbox, inbox: &crate::outbox::Inbox) -> u32 {
        hub.event(Event::Open { peer, page: false, outbox });
        hub.event(Event::Message { peer, message: json!({"type": "join", "version": 12, "name": name}) });
        let welcome: Value = inbox.lock().unwrap().iter().map(|t| serde_json::from_str::<Value>(t).unwrap()).find(|v| v["type"] == "welcome").unwrap();
        welcome["id"].as_u64().unwrap() as u32
    }
    fn steps(hub: &mut Hub, n: usize) {
        for _ in 0..n {
            if hub.advance() {
                hub.send();
            }
        }
    }

    /// Two players send their own views; one sends a batch that does not read halfway through. The good one is kept
    /// beside the match, on its clock; the bad one is dropped there with a note of why; the player who sent it plays
    /// on, its socket open and sent every frame; and the match's own stream is recorded whole. (`Replays::pov` is
    /// handed nothing of the game: it cannot change it. The bots' aim varies from one server to another, so two
    /// servers' matches are not compared frame for frame.)
    #[test]
    fn a_players_own_view_is_kept_beside_the_match_and_a_bad_one_is_dropped_without_touching_it() {
        let dir = folder("views");
        let mut hub = demo_hub(&dir);
        let (nell, nell_in, _) = Outbox::memories();
        let a = joined(&mut hub, 1, "Nell", nell, &nell_in);
        let (otto, otto_out, mut otto_rx) = Outbox::socket();
        hub.event(Event::Open { peer: 2, page: false, outbox: otto });
        hub.event(Event::Message { peer: 2, message: json!({"type": "join", "version": 12, "name": "Otto"}) });
        steps(&mut hub, 30);
        let mut b = 0;
        while let Ok(m) = otto_rx.try_recv() {
            if let crate::outbox::Out::Text(t) = m
                && let Ok(v) = serde_json::from_str::<Value>(t.as_str())
                && v["type"] == "welcome"
            {
                b = v["id"].as_u64().unwrap() as u32;
            }
        }
        assert!(a != 0 && b != 0 && a != b);
        hub.event(Event::Message { peer: 1, message: header(a) });
        hub.event(Event::Message { peer: 2, message: header(b) });
        let mut frames_after = 0;
        for second in 1..=10u32 {
            steps(&mut hub, 60);
            let to = server_ms(&hub) - 100.;
            hub.event(Event::Message { peer: 1, message: batch(second, to, 60) });
            let otto_says = if second == 4 { json!({"type": "pov", "seq": second, "data": "bm90IGEgYmF0Y2g="}) } else { batch(second, to, 60) };
            hub.event(Event::Message { peer: 2, message: otto_says });
            while let Ok(m) = otto_rx.try_recv() {
                frames_after += (second > 4 && matches!(m, crate::outbox::Out::Binary(_))) as usize;
            }
        }
        // Otto is still played and sent every frame after its view was dropped (30 a second).
        assert!(frames_after >= 6 * 30 - 2 && !otto_out.killed.load(std::sync::atomic::Ordering::Relaxed), "{frames_after} frames, socket open");
        assert_eq!((hub.replays.views_kept, hub.replays.views_refused), (10 + 3, 7));
        hub.event(Event::Closed { peer: 1 });
        hub.event(Event::Closed { peer: 2 });
        while !hub.advance() {}
        hub.send();
        // The file: Nell's view whole, Otto's up to the bad batch and why it stops; the match's stream whole.
        let listed = list(&dir);
        assert_eq!(listed.len(), 1);
        let replay = Replay::open(&path(&dir, &listed[0].id).unwrap()).unwrap();
        let ids: Vec<u32> = replay.povs.keys().copied().collect();
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert_eq!(listed[0].views, ids);
        let nell = replay.track(a).unwrap();
        assert_eq!((nell.samples.len(), nell.events.len(), nell.dropped.clone()), (600, 10, None));
        // On the replay's clock: the last batch ends 100 ms before the server's clock when it came.
        let (from, to) = nell.span().unwrap();
        assert!(from > 0. && (to - (replay.duration() as f64 - 100.)).abs() < 40., "{from}..{to} of {}", replay.duration());
        let otto = replay.track(b).unwrap();
        assert_eq!(otto.samples.len(), 180);
        assert!(otto.dropped.as_deref().is_some_and(|w| w.contains("inflate")), "{:?}", otto.dropped);
        let (mut head, played) = Playhead::seek(&replay, 0).unwrap();
        let frames = played.len() + head.advance(&replay, u32::MAX).len();
        assert!(head.done(&replay) && head.stopped.is_none() && frames > 300, "{frames} records");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What breaks a rule is refused and drops that player's view for the rest of the recording: a header that is not
    /// its own, a batch before its header, batches too fast, a batch from the future, more than the cap allows.
    #[test]
    fn a_view_too_fast_too_large_out_of_the_match_or_not_its_own_is_refused() {
        let cases: [(&str, fn(&mut Hub, u32) -> Vec<bool>); 5] = [
            ("its own", |hub, a| vec![hub.replays.pov(hub_game(hub), hub.tick, 60, a, &header(a + 1))]),
            ("before its header", |hub, a| vec![hub.replays.pov(hub_game(hub), hub.tick, 60, a, &batch(1, server_ms(hub) - 100., 30))]),
            ("too fast", |hub, a| {
                let mut kept = vec![hub.replays.pov(hub_game(hub), hub.tick, 60, a, &header(a))];
                for k in 0..12 {
                    kept.push(hub.replays.pov(hub_game(hub), hub.tick, 60, a, &batch(k, server_ms(hub) - 100. - (12 - k) as f64, 1)));
                }
                kept
            }),
            ("outside the match", |hub, a| vec![hub.replays.pov(hub_game(hub), hub.tick, 60, a, &header(a)), hub.replays.pov(hub_game(hub), hub.tick, 60, a, &batch(1, server_ms(hub) + 9000., 30))]),
            ("may keep", |hub, a| {
                let mut kept = vec![hub.replays.pov(hub_game(hub), hub.tick, 60, a, &header(a))];
                // Batches of noise near the most a message may hold, two a second: far more than a screen's view is.
                for k in 0..120u32 {
                    steps(hub, 30);
                    let mut n = 480;
                    let b = loop {
                        let mut seed = 0x9e37_79b9_u64 ^ k as u64;
                        let mut noise = || {
                            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                            (seed >> 40) as f32 / (1u64 << 24) as f32
                        };
                        let samples: Vec<pov::Sample> = (0..n)
                            .map(|i| pov::Sample { t: server_ms(hub) - 4100. + i as f64 * 1000. / 120., now: i as f64, cam: [noise() * 9., noise() * 9., noise() * 9.], turn: [noise() * 3., noise(), noise()], at: [noise() * 9., noise() * 9., noise() * 9.], hold: [noise() * 0.1, noise() * 0.1, noise() * 0.1], look: [noise() * 99., noise() * 99.], ..Default::default() })
                            .collect();
                        let b = pov::Batch { samples, events: vec![] }.encode();
                        if b.len() <= pov::BATCH {
                            break b;
                        }
                        n -= 20;
                    };
                    kept.push(hub.replays.pov(hub_game(hub), hub.tick, 60, a, &json!({"type": "pov", "seq": k, "data": pov::base64(&b)})));
                    if !kept.last().unwrap() {
                        break;
                    }
                }
                assert!(kept.len() > 40 && kept[..kept.len() - 1].iter().all(|k| *k), "kept up to the cap: {}", kept.len());
                kept
            }),
        ];
        for (name, case) in cases {
            let dir = folder(&format!("refuse-{}", name.replace(' ', "-")));
            let mut hub = demo_hub(&dir);
            let (outbox, inbox, _) = Outbox::memories();
            let a = joined(&mut hub, 1, "Nell", outbox, &inbox);
            steps(&mut hub, 60);
            assert!(hub.replays.recording().is_some(), "{name}");
            let kept = case(&mut hub, a);
            assert!(hub.replays.views_refused > 0, "{name}: {kept:?}");
            // Dropped for the rest of the recording: a good batch after it is refused too.
            let header_kept = hub.replays.pov(hub_game(&hub), hub.tick, 60, a, &header(a));
            steps(&mut hub, 30);
            let after = hub.replays.pov(hub_game(&hub), hub.tick, 60, a, &batch(99, server_ms(&hub) - 100., 30));
            assert!(!header_kept && !after, "{name}");
            hub.event(Event::Closed { peer: 1 });
            while !hub.advance() {}
            hub.send();
            let replay = Replay::open(&path(&dir, &list(&dir)[0].id).unwrap()).unwrap();
            assert!(replay.povs.get(&a).is_none_or(|i| i.dropped.as_deref().is_some_and(|w| w.contains(name))), "{name}: {:?}", replay.povs.get(&a).map(|i| i.dropped.clone()));
            // (A view with no batch kept is not in the file's list of views; one dropped after some is, with why.)
            assert_eq!(replay.povs.contains_key(&a), name == "may keep" || name == "too fast", "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    fn hub_game(hub: &Hub) -> u64 {
        hub.game()
    }

    #[test]
    fn the_folder_keeps_the_newest_and_deletes_the_oldest_first() {
        let dir = folder("keep");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..6 {
            let path = dir.join(format!("2026010{i}-000000-map.replay"));
            std::fs::write(&path, b"x").unwrap();
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000 + i * 60);
            File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), b"not a replay").unwrap();
        let r = Replays::new(dir.clone(), 3);
        let mut left: Vec<String> = files(&dir).iter().map(|(p, _)| p.file_name().unwrap().to_string_lossy().into()).collect();
        left.sort();
        assert_eq!(left, vec!["20260103-000000-map.replay", "20260104-000000-map.replay", "20260105-000000-map.replay"]);
        assert!(dir.join("notes.txt").exists(), "only replays are touched");
        drop(r);
        // Off: nothing recorded, nothing deleted.
        let off = Replays::new(dir.clone(), 0);
        assert!(off.dir.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_dates() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(iso(1_763_472_896), "2025-11-18T13:34:56Z");
        assert_eq!(slug("Counter Siege!"), "counter-siege");
        assert!(id("20261118-120000-counter-siege-2") && !id("../x") && !id("a.b") && !id(""));
        assert_eq!(ms(160, 100, 60), 1000);
    }
}

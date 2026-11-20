// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Replays: a game as a client received it, kept in a file to be watched again (format 2; format 1 is read too).
//!
//! **What is kept.** Not the players' inputs but the stream a client is sent: every frame (net.rs, protocol 12) and
//! every JSON text message beside them (`{type:'kungfu'}`, `'titan'`, `'cards'`, `'griddle'`, `'ammo'`, `'tuning'`, `'map'`),
//! each with the moment it was sent, in milliseconds from the recording's start. Played back through the client's own
//! frame code, it looks as it did, whatever the simulation's tuning is by then; and the server (crates/server), the
//! single-player client (which encodes its own `Feast` into the same frames, `snapshot`) and the C interface share it.
//!
//! The server sends each client a frame of its own: the same shared state, encoded against what that client last
//! acknowledged, with its own ack and own body (`Mine`). The shared state is all a spectator needs, so a recording is
//! one neutral stream: each frame encoded against the frame before it, with no ack and no body.
//!
//! **The file.**
//!
//! ```text
//! "HOTDOGRP" (8 bytes) | format (u32, little-endian)
//! block*:  kind (u8) | length of the rest (u32) | the rest
//!   HEADER  zlib(JSON Header)                                   first, always
//!   CHUNK   start ms (u32) | end ms (u32) | markers | zlib(body)  a few seconds of the game
//!   END     zlib(JSON Summary)                                  last, when the recording was finished
//!   POV_HEADER  player (varint) | offset (zigzag, 1/64 ms) | zlib(JSON pov::Header)    format 2: a player's own view
//!   POV         player (varint) | a batch of its track, zlib'd as the client sent it (pov.rs)
//!   POV_DROP    player (varint) | why (text): the server stopped keeping that player's track here
//! markers: count (varint), each: ms (varint) | kind (u8) | by (varint) | to (varint)
//! body:    context: count (varint), each a text (varint length, UTF-8)
//!          tally at the chunk's start: a text (JSON `Tally`)
//!          records: count (varint), each: kind (u8: 0 text, 1 frame) | ms (varint) | length (varint) | bytes
//! ```
//!
//! Each chunk begins with a whole frame (a keyframe, every `KEYFRAME_MS` of play) and the latest text of each kind as
//! it stood then (its context), so it can be played on its own: a seek inflates one chunk, applies its context and
//! plays its frames up to the moment asked for, and never depends on what was played before. The frames after the
//! first are each encoded against the one before (net.rs `encode_frame`). Chunks are written as they close, so a file
//! cut short (a server that crashed) still holds every chunk but the last, and plays to where it ends; the END block
//! is only a summary (the duration, the winner, the final tally) that a listing reads without inflating the rest.
//!
//! **Players' own views** (format 2, pov.rs): beside the neutral stream, a file may hold a track for any player of what
//! its own screen showed (the camera as drawn, the gun in hand, what its display heard), sent by its client while the
//! match was played (or written by single player's own recorder). Each track's blocks are written as they come,
//! between the chunks; its header says where the recording began on the track's clock (`offset`), so its samples are
//! put on the replay's. A format-1 file has none, and plays as before.
//! Compression is zlib (deflate, miniz_oxide: pure Rust, also in the browser build) per block, whose checksum catches
//! a damaged chunk. A file of another format is refused with a clear message (`Error::Format`); one from another build
//! or protocol is played with a warning (`Replay::warning`).
//!
//! **Stats** (`Tally`) are worked out from the frames as they are played, by the writer and the reader alike, and the
//! writer keeps the tally at each chunk's start in it, so a seek has the stats at once. Markers (kills, Kung Fu KOs, the
//! end of a round) are kept beside each chunk, uncompressed, so the timeline has them all without inflating anything.
use crate::feast::{Feast, Stage};
use crate::net::{self, Choice, Events, Frame, Loose, Malformed, Seat, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom, Write};

/// The first bytes of a replay file.
pub const MAGIC: &[u8; 8] = b"HOTDOGRP";
/// The format this build writes, and the oldest it reads (format 1: before players' own views).
pub const FORMAT: u32 = 2;
pub const OLDEST: u32 = 1;
/// A replay file's extension.
pub const EXTENSION: &str = "replay";
/// Milliseconds of play between keyframes: what a seek decodes at most.
pub const KEYFRAME_MS: u32 = 5000;
/// The most one chunk may inflate to: a damaged or hostile file cannot make the reader take more memory than this.
pub const CHUNK_LIMIT: usize = 32 << 20;
/// The most a header or a summary may inflate to.
const META_LIMIT: usize = 8 << 20;
/// The text kinds a chunk's context keeps the latest of.
pub const CONTEXT: [&str; 7] = ["map", "tuning", "cards", "griddle", "kungfu", "ammo", "titan"];

const HEADER: u8 = 1;
const CHUNK: u8 = 2;
const END: u8 = 3;
const POV_HEADER: u8 = 4;
const POV: u8 = 5;
const POV_DROP: u8 = 6;
const TEXT: u8 = 0;
const FRAME: u8 = 1;

// ----- What a file says of itself.

/// Who played, as the recording began.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Member {
    pub id: u32,
    pub name: String,
    pub team: u8,
    pub bot: bool,
    pub color: String,
}
/// The header: what the recording is of.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Header {
    pub format: u32,
    pub protocol: u32,
    /// The program and version that wrote it ("hotdog-server 0.1.0", "hotdog-simulator 0.1.0").
    pub build: String,
    /// "server" or "offline".
    pub source: String,
    /// The map's name and its whole text (the editor's JSON): maps change, the replay keeps the one it was played on.
    pub map_name: String,
    pub map: String,
    /// The mode the game was played in (siege, forts, ffa, cards, kungfu: rules.rs `Mode`, the game's and not the map's,
    /// so a Forts map played as a siege says siege) and the rules (rules.rs `Rules::text`, which name the mode where it
    /// is not the map's own).
    pub mode: String,
    pub rules: String,
    pub roster: Vec<Member>,
    /// Unix seconds the recording began.
    pub started: u64,
    /// Steps and frames a second of the game recorded.
    pub tick_hz: u32,
    pub send_hz: u32,
    pub keyframe_ms: u32,
    /// A server's recording: where it began on the server's clock (ms since the server started, its steps'), the clock
    /// its players' own views are kept on (pov.rs); 0 in a file from before it was kept.
    pub start_ms: f64,
}
impl Header {
    /// A header for a recording of `map` (its text), by `build`, from `source`.
    pub fn new(build: &str, source: &str, map: &crate::map::Map, mode: &str, rules: &str, roster: Vec<Member>, started: u64) -> Header {
        Header {
            format: FORMAT,
            protocol: net::PROTOCOL,
            build: build.into(),
            source: source.into(),
            map_name: map.name.clone(),
            map: map.to_text(),
            mode: mode.into(),
            rules: rules.into(),
            roster,
            started,
            tick_hz: net::TICK_HZ,
            send_hz: net::SEND_HZ,
            keyframe_ms: KEYFRAME_MS,
            start_ms: 0.,
        }
    }
    /// Whether the recording is of Kung Fu (its KOs are markers of their own).
    pub fn kungfu(&self) -> bool {
        self.mode == crate::kungfu::MODE || self.rules.split(' ').any(|w| w == "mode=kungfu")
    }
}
/// The roster of a frame's hotdogs, for a header.
pub fn roster(frame: &Frame) -> Vec<Member> {
    frame.players.iter().map(|s| Member { id: s.player.id, name: s.player.name.clone(), team: s.player.team, bot: s.player.bot, color: s.color.clone() }).collect()
}
/// What the END block says: how long, who won, and the final tally.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub duration_ms: u32,
    /// Unix seconds it ended.
    pub ended: u64,
    pub winner: Option<String>,
    pub lines: Vec<Line>,
    pub markers: u32,
}

// ----- Markers and stats.

/// Something to find on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mark {
    Kill = 1,
    /// A knockout in Kung Fu.
    Ko = 2,
    /// The end of a round (`by` is the winner's id + 1, or 0).
    RoundEnd = 3,
}
impl Mark {
    fn from(n: u8) -> Option<Mark> {
        match n {
            1 => Some(Mark::Kill),
            2 => Some(Mark::Ko),
            3 => Some(Mark::RoundEnd),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub ms: u32,
    pub kind: Mark,
    /// The killer and the victim (0: nobody: a cat, the kitchen, a fall).
    pub by: u32,
    pub to: u32,
}
/// What the last frame said of a hotdog, to see what has changed since.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub alive: bool,
    pub kills: u32,
    pub swings: u32,
    /// Its last damage: kind, attacker, the round's clock it landed at (ms) and the HP it took (hundredths).
    pub hit: (u8, u32, i64, i64),
}
/// One hotdog's line of stats, from the start of the recording to the moment played.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Line {
    pub id: u32,
    pub name: String,
    pub team: u8,
    pub bot: bool,
    /// Kills and deaths in the whole recording (a counter that starts again with a round is added up), and the score
    /// as the newest frame has it.
    pub kills: u32,
    pub deaths: u32,
    pub score: u32,
    /// Rounds fired (each a shot the frames tell of), melee swings, and hits: damage it did to another hotdog.
    pub shots: u32,
    pub swings: u32,
    pub hits: u32,
    /// Damage done to other hotdogs and taken from anything, in hundredths of HP.
    pub dealt: u64,
    pub taken: u64,
    /// Kills since its last death, and the most in one life.
    pub streak: u32,
    pub best: u32,
    /// As the newest frame has it: alive, HP, the weapon in hand and its magazine.
    pub alive: bool,
    pub hp: f32,
    pub weapon: usize,
    pub magazine: i32,
    pub seen: Seen,
}
impl Line {
    /// Hits for every attack made (rounds fired and swings), when it has made any.
    pub fn accuracy(&self) -> Option<f32> {
        let attacks = self.shots + self.swings;
        (attacks > 0).then(|| self.hits as f32 / attacks as f32)
    }
}
/// Damage the last frame observed told of: who did it (0: nothing that is a hotdog), to whom, of what kind, how much.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub by: u32,
    pub to: u32,
    pub kind: u8,
    pub amount: f32,
}
/// The stats of a recording as far as it has been played: a line per hotdog seen (see the module's documentation).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub kungfu: bool,
    pub lines: Vec<Line>,
    pub round: u32,
    pub stage: String,
    /// The damage the last frame observed told of (for a viewer's damage numbers; not kept in a file).
    #[serde(skip)]
    pub hits: Vec<Hit>,
}
impl PartialEq for Tally {
    /// The stats alike (what the last frame told of aside).
    fn eq(&self, other: &Tally) -> bool {
        (self.kungfu, &self.lines, self.round, &self.stage) == (other.kungfu, &other.lines, other.round, &other.stage)
    }
}
impl Tally {
    pub fn new(kungfu: bool) -> Tally {
        Tally { kungfu, ..Default::default() }
    }
    pub fn line(&self, id: u32) -> Option<&Line> {
        self.lines.iter().find(|l| l.id == id)
    }
    fn line_mut(&mut self, id: u32) -> &mut Line {
        match self.lines.iter().position(|l| l.id == id) {
            Some(i) => &mut self.lines[i],
            None => {
                self.lines.push(Line { id, ..Default::default() });
                self.lines.last_mut().unwrap()
            }
        }
    }
    /// A frame played at `ms` (decoded, with its events): the stats move on, and the markers it makes are returned.
    pub fn observe(&mut self, ms: u32, frame: &Frame) -> Vec<Marker> {
        let mut marks = vec![];
        self.hits.clear();
        // A round ends: the stage turns to RoundOver.
        if frame.stage == "RoundOver" && self.stage != "RoundOver" && !self.stage.is_empty() {
            marks.push(Marker { ms, kind: Mark::RoundEnd, by: frame.winner.map_or(0, |w| w + 1), to: frame.round });
        }
        let new_round = frame.round != self.round;
        (self.round, self.stage) = (frame.round, frame.stage.clone());
        for shot in frame.shots.iter().filter(|s| s.projectile != 0 && s.owner != 0) {
            self.line_mut(shot.owner).shots += 1;
        }
        let (mut dealt, mut hits): (Vec<(u32, i64)>, Vec<Hit>) = (vec![], vec![]);
        for seat in &frame.players {
            let p = &seat.player;
            let hit = (p.last_damage.kind, p.last_damage.attacker, (p.last_damage.time as f64 * 1e3).round() as i64, (p.last_damage.amount as f64 * 1e2).round() as i64);
            let kungfu = self.kungfu;
            let fresh = !self.lines.iter().any(|l| l.id == p.id);
            let line = self.line_mut(p.id);
            (line.name, line.team, line.bot) = (p.name.clone(), p.team, p.bot);
            (line.score, line.hp, line.weapon) = (p.score, p.hp, p.weapon);
            line.magazine = p.ammo.get(p.weapon).copied().unwrap_or(0);
            if fresh {
                // First seen: what it already has is where it starts (a recording can begin mid-match).
                line.seen = Seen { alive: p.alive, kills: p.kills, swings: p.swings, hit };
                line.alive = p.alive;
                continue;
            }
            // Counters that start again with a round (or a new seat) start from where they are now.
            let kills = if p.kills >= line.seen.kills { p.kills - line.seen.kills } else { p.kills };
            let swings = if p.swings >= line.seen.swings { p.swings - line.seen.swings } else { p.swings };
            line.kills += kills;
            line.streak += kills;
            line.best = line.best.max(line.streak);
            line.swings += swings;
            if hit != line.seen.hit && hit.3 > 0 && !(new_round && hit.2 == 0) {
                line.taken += hit.3 as u64;
                hits.push(Hit { by: hit.1, to: p.id, kind: hit.0, amount: hit.3 as f32 / 100. });
                if hit.1 != 0 && hit.1 != p.id {
                    dealt.push((hit.1, hit.3));
                }
            }
            if line.seen.alive && !p.alive {
                line.deaths += 1;
                line.streak = 0;
                marks.push(Marker { ms, kind: if kungfu { Mark::Ko } else { Mark::Kill }, by: p.last_damage.attacker, to: p.id });
            }
            line.alive = p.alive;
            line.seen = Seen { alive: p.alive, kills: p.kills, swings: p.swings, hit };
        }
        for (by, amount) in dealt {
            let line = self.line_mut(by);
            line.hits += 1;
            line.dealt += amount as u64;
        }
        self.hits = hits;
        marks
    }
}

// ----- Errors.

/// Why a file cannot be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a replay at all.
    NotAReplay,
    /// A replay of another format.
    Format { found: u32 },
    /// Damaged where it cannot be played past (the header, or the chunk named).
    Damaged(String),
    Io(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotAReplay => write!(f, "This is not a Hotdog Simulator replay."),
            Error::Format { found } if *found > FORMAT => write!(f, "This replay is format {found}, newer than this game reads (format {FORMAT}): update the game to watch it."),
            Error::Format { found } => write!(f, "This replay is format {found}, older than this game reads (formats {OLDEST} to {FORMAT}): it cannot be played by this build."),
            Error::Damaged(what) => write!(f, "The replay is damaged: {what}."),
            Error::Io(what) => write!(f, "The replay could not be read: {what}."),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e.to_string())
    }
}

// ----- Bytes.

fn varint(out: &mut Vec<u8>, v: u64) {
    let mut w = net::Writer::new();
    w.varint(v);
    out.extend_from_slice(&w.bytes);
}
fn text(out: &mut Vec<u8>, s: &str) {
    varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}
fn deflate(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}
fn inflate(bytes: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, limit).map_err(|_| Error::Damaged("a block does not inflate".into()))
}
fn write_block(out: &mut impl Write, kind: u8, body: &[u8]) -> io::Result<u64> {
    let mut head = [0u8; 5];
    head[0] = kind;
    head[1..].copy_from_slice(&(body.len() as u32).to_le_bytes());
    out.write_all(&head)?;
    out.write_all(body)?;
    out.flush()?;
    Ok(5 + body.len() as u64)
}
fn write_markers(out: &mut Vec<u8>, markers: &[Marker]) {
    varint(out, markers.len() as u64);
    for m in markers {
        varint(out, m.ms as u64);
        out.push(m.kind as u8);
        varint(out, m.by as u64);
        varint(out, m.to as u64);
    }
}
// ----- Writing.

/// A chunk being written.
struct Open {
    start: u32,
    end: u32,
    context: Vec<String>,
    tally: Tally,
    markers: Vec<Marker>,
    records: Vec<u8>,
    count: u64,
}
/// Writes a recording as it is made (see the module's documentation): the header at once, each chunk when it closes,
/// the summary at `finish`.
pub struct Writer<W: Write> {
    out: W,
    open: Option<Open>,
    keyframe_ms: u32,
    /// The newest frame written (its step and state): the next is encoded against it.
    last: Option<(u32, State)>,
    /// The newest shot, splat and knock told, and the round they were numbered in.
    told: [u32; 3],
    round: Option<u32>,
    /// The latest text of each kind (`CONTEXT`).
    context: BTreeMap<String, String>,
    tally: Tally,
    ms: u32,
    winner: Option<String>,
    markers: u32,
    written: u64,
}
/// The kind of a JSON text message (its `type`).
pub fn kind_of(text: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }
    serde_json::from_str::<Kind>(text).ok().map(|k| k.kind)
}
impl<W: Write> Writer<W> {
    /// Begins a recording into `out`: the magic, the format and the header are written at once.
    pub fn new(mut out: W, header: &Header) -> io::Result<Writer<W>> {
        let mut head = MAGIC.to_vec();
        head.extend_from_slice(&FORMAT.to_le_bytes());
        out.write_all(&head)?;
        let json = serde_json::to_vec(header).unwrap_or_default();
        let written = head.len() as u64 + write_block(&mut out, HEADER, &deflate(&json))?;
        let keyframe_ms = if header.keyframe_ms > 0 { header.keyframe_ms } else { KEYFRAME_MS };
        Ok(Writer {
            out,
            open: None,
            keyframe_ms,
            last: None,
            told: [0; 3],
            round: None,
            context: BTreeMap::new(),
            tally: Tally::new(header.kungfu()),
            ms: 0,
            winner: None,
            markers: 0,
            written,
        })
    }
    /// The milliseconds recorded so far, and the bytes written so far.
    pub fn duration(&self) -> u32 {
        self.ms
    }
    pub fn written(&self) -> u64 {
        self.written
    }
    /// What has been written (an in-memory recording's bytes so far).
    pub fn get_ref(&self) -> &W {
        &self.out
    }
    pub fn tally(&self) -> &Tally {
        &self.tally
    }
    /// A text message as a client was sent it at `ms`.
    pub fn text(&mut self, ms: u32, message: &str) {
        let ms = ms.max(self.ms);
        if let Some(kind) = kind_of(message).filter(|k| CONTEXT.contains(&k.as_str())) {
            self.context.insert(kind, message.to_string());
        }
        if let Some(open) = &mut self.open {
            open.records.push(TEXT);
            varint(&mut open.records, ms as u64);
            text(&mut open.records, message);
            open.count += 1;
            open.end = ms;
            self.ms = ms;
        }
    }
    /// A frame of step `tick` at `ms`: `frame` as the server made it (its lists of shots, splats and knocks are the
    /// round's; only those not yet told go out) and its `state` (`frame.state()`). A frame no newer than the last is
    /// passed over. A chunk that has run KEYFRAME_MS is closed first and written, and this frame begins the next whole.
    pub fn frame(&mut self, ms: u32, tick: u32, frame: &Frame, state: &State) -> io::Result<()> {
        if self.last.as_ref().is_some_and(|(t, _)| tick <= *t) {
            return Ok(());
        }
        let ms = ms.max(self.ms);
        if self.round != Some(frame.round) {
            (self.round, self.told) = (Some(frame.round), [0; 3]);
        }
        let events = Events {
            shots: frame.shots.iter().filter(|s| s.id > self.told[0]).cloned().collect(),
            splats: frame.splats.iter().filter(|s| s.id > self.told[1]).cloned().collect(),
            knocks: frame.knocks.iter().filter(|k| k.id > self.told[2]).cloned().collect(),
        };
        self.told = [
            events.shots.iter().map(|s| s.id).fold(self.told[0], u32::max),
            events.splats.iter().map(|s| s.id).fold(self.told[1], u32::max),
            events.knocks.iter().map(|k| k.id).fold(self.told[2], u32::max),
        ];
        if self.open.as_ref().is_none_or(|o| ms >= o.start + self.keyframe_ms) {
            self.close()?;
            let context = CONTEXT.iter().filter_map(|k| self.context.get(*k).cloned()).collect();
            self.open = Some(Open { start: ms, end: ms, context, tally: self.tally.clone(), markers: vec![], records: vec![], count: 0 });
            self.last = None;
        }
        let base = self.last.as_ref();
        let header = net::Header { tick, base: base.map_or(0, |(t, _)| *t), ack: 0, checksum: state.checksum(), elapsed: frame.elapsed, mine: None };
        let bytes = net::encode_frame(&header, base.map(|(_, s)| s), state, &events);
        // The stats from the frame as a player of the file will decode it.
        let mut seen = Frame::from_state(state, frame.elapsed);
        (seen.shots, seen.splats, seen.knocks) = (events.shots, events.splats, events.knocks);
        let marks = self.tally.observe(ms, &seen);
        if let Some(name) = winner(&seen) {
            self.winner = Some(name);
        }
        let open = self.open.as_mut().unwrap();
        self.markers += marks.len() as u32;
        open.markers.extend(marks);
        open.records.push(FRAME);
        varint(&mut open.records, ms as u64);
        varint(&mut open.records, bytes.len() as u64);
        open.records.extend_from_slice(&bytes);
        open.count += 1;
        open.end = ms;
        self.ms = ms;
        self.last = Some((tick, state.clone()));
        Ok(())
    }
    /// A player's own view begins (or begins again in this file): its header (`pov::Header`'s JSON), and where this
    /// recording began on the track's clock (ms).
    pub fn pov_header(&mut self, player: u32, offset_ms: f64, header: &str) -> io::Result<()> {
        let mut block = vec![];
        varint(&mut block, player as u64);
        let mut w = net::Writer::new();
        w.zigzag((offset_ms * 64.).round() as i64);
        block.extend_from_slice(&w.bytes);
        block.extend_from_slice(&deflate(header.as_bytes()));
        self.written += write_block(&mut self.out, POV_HEADER, &block)?;
        Ok(())
    }
    /// A batch of a player's own view (`pov::Batch`, zlib'd as its client sent it).
    pub fn pov(&mut self, player: u32, batch: &[u8]) -> io::Result<()> {
        let mut block = vec![];
        varint(&mut block, player as u64);
        block.extend_from_slice(batch);
        self.written += write_block(&mut self.out, POV, &block)?;
        Ok(())
    }
    /// A player's own view is kept no further in this file, and why.
    pub fn pov_drop(&mut self, player: u32, why: &str) -> io::Result<()> {
        let mut block = vec![];
        varint(&mut block, player as u64);
        text(&mut block, why);
        self.written += write_block(&mut self.out, POV_DROP, &block)?;
        Ok(())
    }
    /// Closes the chunk being written, if any, and writes it.
    fn close(&mut self) -> io::Result<()> {
        let Some(o) = self.open.take() else { return Ok(()) };
        let mut body = vec![];
        varint(&mut body, o.context.len() as u64);
        for t in &o.context {
            text(&mut body, t);
        }
        text(&mut body, &serde_json::to_string(&o.tally).unwrap_or_default());
        varint(&mut body, o.count);
        body.extend_from_slice(&o.records);
        let mut block = vec![];
        block.extend_from_slice(&o.start.to_le_bytes());
        block.extend_from_slice(&o.end.to_le_bytes());
        write_markers(&mut block, &o.markers);
        block.extend_from_slice(&deflate(&body));
        self.written += write_block(&mut self.out, CHUNK, &block)?;
        Ok(())
    }
    /// Writes what is still open and the summary (it ended at Unix second `ended`); returns the output and the summary.
    pub fn finish(mut self, ended: u64) -> io::Result<(W, Summary)> {
        self.close()?;
        let summary = Summary { duration_ms: self.ms, ended, winner: self.winner.clone(), lines: self.tally.lines.clone(), markers: self.markers };
        let json = serde_json::to_vec(&summary).unwrap_or_default();
        self.written += write_block(&mut self.out, END, &deflate(&json))?;
        Ok((self.out, summary))
    }
}
/// Who won, by what a frame says: the last hotdog standing (by name), or in Forts the team.
fn winner(frame: &Frame) -> Option<String> {
    if let Some(team) = frame.forts.as_ref().and_then(|f| f.winner) {
        return crate::map::TEAM_NAMES.get(team as usize).map(|n| n.to_string());
    }
    let id = frame.winner?;
    frame.players.iter().find(|s| s.player.id == id).map(|s| s.player.name.clone())
}

// ----- Reading.

/// Where a chunk is in a file, the stretch of play it holds and its markers.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkIndex {
    pub start: u32,
    pub end: u32,
    pub markers: Vec<Marker>,
    at: usize,
    len: usize,
}
/// One record: a text message or a frame (its bytes as a client was sent them).
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Text(String),
    Frame(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub ms: u32,
    pub record: Record,
}
/// A chunk inflated: its context, the tally at its start, its markers and its records.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub start: u32,
    pub end: u32,
    pub context: Vec<String>,
    pub tally: Tally,
    pub markers: Vec<Marker>,
    pub records: Vec<Timed>,
}
/// Where a player's own view is in a file: its header, the recording's start on its clock (ms), its batches, and why
/// the server stopped keeping it, if it did.
#[derive(Clone, Debug, Default)]
pub struct PovIndex {
    pub header: Option<crate::pov::Header>,
    pub offset: f64,
    batches: Vec<(usize, usize)>,
    pub dropped: Option<String>,
}
impl PovIndex {
    /// How many batches it has.
    pub fn batches(&self) -> usize {
        self.batches.len()
    }
}
/// A replay file read: the header, the summary if it was finished, and the chunks it holds (inflated one at a time,
/// when they are played: the file's own bytes are all that is kept).
pub struct Replay {
    pub header: Header,
    pub summary: Option<Summary>,
    pub chunks: Vec<ChunkIndex>,
    /// The players' own views it holds (format 2), by player.
    pub povs: BTreeMap<u32, PovIndex>,
    /// Cut short: no summary (a recording not finished), or a block that runs past the end.
    pub truncated: bool,
    data: Vec<u8>,
}
/// What a listing says of a file, read without inflating its chunks (`scan`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub header: Header,
    pub summary: Option<Summary>,
    pub duration_ms: u32,
    pub truncated: bool,
    /// The players whose own view it holds.
    #[serde(default)]
    pub povs: Vec<u32>,
}
fn check_magic(head: &[u8]) -> Result<(), Error> {
    if head.len() < 12 || &head[..8] != MAGIC {
        return Err(Error::NotAReplay);
    }
    let found = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    if !(OLDEST..=FORMAT).contains(&found) {
        return Err(Error::Format { found });
    }
    Ok(())
}
/// A POV block's player, and where the rest of it begins.
fn pov_player(block: &[u8]) -> Option<(u32, usize)> {
    let mut r = net::Reader::new(block);
    let player = u32::try_from(r.varint().ok()?).ok()?;
    let mut w = net::Writer::new();
    w.varint(player as u64);
    Some((player, w.bytes.len()))
}
impl Replay {
    /// Reads a replay from its bytes. A file cut short reads up to its last whole chunk.
    pub fn parse(data: Vec<u8>) -> Result<Replay, Error> {
        check_magic(&data)?;
        let mut at = 12;
        let (mut header, mut summary, mut chunks, mut truncated) = (None, None, vec![], false);
        let mut povs: BTreeMap<u32, PovIndex> = BTreeMap::new();
        while at < data.len() {
            if at + 5 > data.len() {
                truncated = true;
                break;
            }
            let kind = data[at];
            let len = u32::from_le_bytes([data[at + 1], data[at + 2], data[at + 3], data[at + 4]]) as usize;
            let body = at + 5;
            if body + len > data.len() {
                truncated = true;
                break;
            }
            let block = &data[body..body + len];
            match kind {
                HEADER => {
                    let json = inflate(block, META_LIMIT)?;
                    header = Some(serde_json::from_slice::<Header>(&json).map_err(|e| Error::Damaged(format!("its header does not read ({e})")))?);
                }
                CHUNK => match ChunkPrelude::read(block) {
                    Ok(p) => chunks.push(ChunkIndex { start: p.start, end: p.end, markers: p.markers, at: body + p.body, len: len - p.body }),
                    Err(_) => {
                        truncated = true;
                        break;
                    }
                },
                END => summary = inflate(block, META_LIMIT).ok().and_then(|json| serde_json::from_slice::<Summary>(&json).ok()),
                // (A player's own view that does not read is passed over: the replay plays without it.)
                POV_HEADER => {
                    if let Some((player, rest)) = pov_player(block) {
                        let mut r = net::Reader::new(&block[rest..]);
                        let offset = r.zigzag().ok().map(|o| o as f64 / 64.);
                        let mut w = net::Writer::new();
                        w.zigzag(offset.map_or(0, |o| (o * 64.) as i64));
                        let json = inflate(&block[(rest + w.bytes.len()).min(block.len())..], crate::pov::HEADER).ok();
                        let parsed = json.and_then(|j| String::from_utf8(j).ok()).and_then(|t| crate::pov::Header::parse(&t).ok());
                        if let (Some(offset), Some(h)) = (offset, parsed) {
                            let index = povs.entry(player).or_default();
                            (index.header, index.offset) = (Some(h), offset);
                        }
                    }
                }
                POV => {
                    if let Some((player, rest)) = pov_player(block)
                        && let Some(index) = povs.get_mut(&player).filter(|i| i.header.is_some() && i.dropped.is_none())
                    {
                        index.batches.push((body + rest, len - rest));
                    }
                }
                POV_DROP => {
                    if let Some((player, rest)) = pov_player(block) {
                        let why = net::Reader::new(&block[rest..]).text().unwrap_or_default();
                        povs.entry(player).or_default().dropped = Some(why);
                    }
                }
                // (A kind this build does not know: passed over.)
                _ => {}
            }
            if header.is_none() {
                return Err(Error::Damaged("it has no header".into()));
            }
            at = body + len;
        }
        let header = header.ok_or_else(|| Error::Damaged("it has no header".into()))?;
        truncated |= summary.is_none();
        povs.retain(|_, i| i.header.is_some() && !i.batches.is_empty());
        Ok(Replay { header, summary, chunks, truncated, data, povs })
    }
    /// Reads a replay file.
    pub fn open(path: &std::path::Path) -> Result<Replay, Error> {
        Replay::parse(std::fs::read(path)?)
    }
    /// How long it plays, in milliseconds.
    pub fn duration(&self) -> u32 {
        self.chunks.last().map_or(0, |c| c.end).max(self.summary.as_ref().map_or(0, |s| s.duration_ms).min(self.chunks.last().map_or(0, |c| c.end)))
    }
    /// Every marker, in order.
    pub fn markers(&self) -> impl Iterator<Item = &Marker> {
        self.chunks.iter().flat_map(|c| c.markers.iter())
    }
    /// The chunk that holds `ms`: the last that starts at or before it.
    pub fn find(&self, ms: u32) -> usize {
        self.chunks.iter().rposition(|c| c.start <= ms).unwrap_or(0)
    }
    /// The chunk `i`, inflated.
    pub fn chunk(&self, i: usize) -> Result<Chunk, Error> {
        let index = self.chunks.get(i).ok_or_else(|| Error::Damaged(format!("there is no chunk {i}")))?;
        let body = inflate(&self.data[index.at..index.at + index.len], CHUNK_LIMIT)?;
        let damaged = |m: Malformed| Error::Damaged(format!("chunk {i} at {} ms: {}", index.start, m.0));
        let mut r = net::Reader::new(&body);
        let mut context = vec![];
        for _ in 0..r.count().map_err(damaged)? {
            context.push(r.text().map_err(damaged)?);
        }
        let tally = serde_json::from_str::<Tally>(&r.text().map_err(damaged)?).unwrap_or_default();
        let mut records = vec![];
        for _ in 0..r.count().map_err(damaged)? {
            let kind = r.u8().map_err(damaged)?;
            let ms = r.varint().map_err(damaged)? as u32;
            let record = match kind {
                TEXT => Record::Text(r.text().map_err(damaged)?),
                FRAME => {
                    let n = r.count().map_err(damaged)?;
                    let mut bytes = Vec::with_capacity(n);
                    for _ in 0..n {
                        bytes.push(r.u8().map_err(damaged)?);
                    }
                    Record::Frame(bytes)
                }
                _ => return Err(Error::Damaged(format!("chunk {i} has a record of kind {kind}"))),
            };
            records.push(Timed { ms, record });
        }
        Ok(Chunk { start: index.start, end: index.end, context, tally, markers: index.markers.clone(), records })
    }
    /// A warning for a file from another build or protocol (it is played anyway), or None.
    pub fn warning(&self, build: &str) -> Option<String> {
        let h = &self.header;
        if h.protocol != net::PROTOCOL {
            return Some(format!("Recorded with protocol {} (this game speaks {}): it may not play as it looked.", h.protocol, net::PROTOCOL));
        }
        if h.build != build && h.build.split(' ').nth(1) != build.split(' ').nth(1) {
            return Some(format!("Recorded by {} (this is {}): it may not play exactly as it looked.", h.build, build));
        }
        None
    }
    /// The bytes it was read from (to save or offer for download).
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    /// Player `player`'s own view, read (its batches inflated and put on the replay's clock), if the file has one.
    pub fn track(&self, player: u32) -> Option<crate::pov::Track> {
        let index = self.povs.get(&player)?;
        let batches = index.batches.iter().map(|(at, len)| &self.data[*at..*at + *len]);
        let mut track = crate::pov::Track::from_batches(index.header.clone()?, index.offset, batches);
        track.dropped = index.dropped.clone();
        (!track.samples.is_empty()).then_some(track)
    }
}
/// A chunk block's uncompressed start: its stretch, its markers and where its zlib body begins.
struct ChunkPrelude {
    start: u32,
    end: u32,
    markers: Vec<Marker>,
    body: usize,
}
impl ChunkPrelude {
    fn read(block: &[u8]) -> Result<ChunkPrelude, Malformed> {
        if block.len() < 8 {
            return Err(Malformed("cut short"));
        }
        let start = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        let end = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
        // The markers' bytes are counted by reading them from a slice that shrinks with each.
        let mut at = 8;
        let take = |at: &mut usize| -> Result<u64, Malformed> {
            let mut r = net::Reader::new(&block[*at..]);
            let v = r.varint()?;
            let mut w = net::Writer::new();
            w.varint(v);
            *at += w.bytes.len();
            Ok(v)
        };
        let n = take(&mut at)?;
        if n > block.len() as u64 {
            return Err(Malformed("a count larger than the block"));
        }
        let mut markers = vec![];
        for _ in 0..n {
            let ms = take(&mut at)? as u32;
            let kind = *block.get(at).ok_or(Malformed("cut short"))?;
            at += 1;
            let by = take(&mut at)? as u32;
            let to = take(&mut at)? as u32;
            if let Some(kind) = Mark::from(kind) {
                markers.push(Marker { ms, kind, by, to });
            }
        }
        Ok(ChunkPrelude { start, end, markers, body: at })
    }
}
/// Reads what a listing needs of a replay (its header, its summary, how long it is, whether it was cut short) without
/// inflating its chunks: the blocks' lengths are stepped over.
pub fn scan<R: Read + Seek>(r: &mut R) -> Result<Listing, Error> {
    let size = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut head = [0u8; 12];
    r.read_exact(&mut head).map_err(|_| Error::NotAReplay)?;
    check_magic(&head)?;
    let mut at = 12u64;
    let mut listing = Listing::default();
    let mut header = None;
    while at + 5 <= size {
        let mut b = [0u8; 5];
        r.seek(SeekFrom::Start(at))?;
        r.read_exact(&mut b)?;
        let len = u32::from_le_bytes([b[1], b[2], b[3], b[4]]) as u64;
        if at + 5 + len > size {
            listing.truncated = true;
            break;
        }
        match b[0] {
            HEADER | END => {
                if len as usize > META_LIMIT {
                    return Err(Error::Damaged("a block too large".into()));
                }
                let mut block = vec![0u8; len as usize];
                r.read_exact(&mut block)?;
                let json = inflate(&block, META_LIMIT)?;
                if b[0] == HEADER {
                    header = Some(serde_json::from_slice::<Header>(&json).map_err(|e| Error::Damaged(format!("its header does not read ({e})")))?);
                } else {
                    listing.summary = serde_json::from_slice::<Summary>(&json).ok();
                }
            }
            CHUNK if len >= 8 => {
                let mut se = [0u8; 8];
                r.read_exact(&mut se)?;
                listing.duration_ms = listing.duration_ms.max(u32::from_le_bytes([se[4], se[5], se[6], se[7]]));
            }
            POV_HEADER if len >= 1 => {
                let mut head = [0u8; 5];
                let n = r.read(&mut head[..(len as usize).min(5)])?;
                if let Some((player, _)) = pov_player(&head[..n])
                    && !listing.povs.contains(&player)
                {
                    listing.povs.push(player);
                }
            }
            _ => {}
        }
        if header.is_none() {
            return Err(Error::Damaged("it has no header".into()));
        }
        at += 5 + len;
    }
    listing.truncated |= at != size || listing.summary.is_none();
    listing.header = header.ok_or_else(|| Error::Damaged("it has no header".into()))?;
    Ok(listing)
}

// ----- Playing.

/// Decodes a recording's frames in order, each against the one before (as a client decodes the server's).
#[derive(Default)]
pub struct Decoder {
    last: Option<(u32, State)>,
}
impl Decoder {
    /// A frame's bytes: its header and the frame it says, with its events; checked against its checksum.
    pub fn frame(&mut self, bytes: &[u8]) -> Result<(net::Header, Frame), Malformed> {
        let (tick, base) = net::peek_frame(bytes)?;
        let state = match base {
            0 => None,
            base => match &self.last {
                Some((t, s)) if *t == base => Some(s),
                _ => return Err(Malformed("a frame against one not played")),
            },
        };
        let (header, state, events) = net::decode_frame(bytes, state)?;
        if state.checksum() != header.checksum {
            return Err(Malformed("a frame that does not check out"));
        }
        let mut frame = Frame::from_state(&state, header.elapsed);
        (frame.tick, frame.ack, frame.mine) = (tick, header.ack, header.mine);
        (frame.shots, frame.splats, frame.knocks) = (events.shots, events.splats, events.knocks);
        self.last = Some((tick, state));
        Ok((header, frame))
    }
    /// The state of the newest frame decoded.
    pub fn state(&self) -> Option<&State> {
        self.last.as_ref().map(|(_, s)| s)
    }
}
/// A record played: when, what, whether it is a chunk's context (a seek's, applied once: not part of the stream), the
/// frame decoded from it, and the markers it made.
pub struct Played {
    pub ms: u32,
    pub record: Record,
    pub context: bool,
    pub frame: Option<Frame>,
    pub markers: Vec<Marker>,
    /// The damage the frame told of.
    pub hits: Vec<Hit>,
}
/// Where a recording is being played: the chunk, the records of it played, the frames' decoder and the stats so far.
pub struct Playhead {
    pub chunk: usize,
    records: Vec<Timed>,
    next: usize,
    pub decoder: Decoder,
    pub tally: Tally,
    /// The time of the newest record played.
    pub ms: u32,
    /// Why playing stopped short of the end (a damaged chunk), if it did.
    pub stopped: Option<String>,
}
impl Playhead {
    /// The recording from `ms` (at its chunk's keyframe): the playhead, and what to apply: that chunk's context, then
    /// every record of it up to `ms`.
    pub fn seek(replay: &Replay, ms: u32) -> Result<(Playhead, Vec<Played>), Error> {
        if replay.chunks.is_empty() {
            return Err(Error::Damaged("it holds no part of a game".into()));
        }
        let i = replay.find(ms);
        let chunk = replay.chunk(i)?;
        let mut head = Playhead { chunk: i, records: chunk.records, next: 0, decoder: Decoder::default(), tally: chunk.tally, ms: chunk.start, stopped: None };
        let mut out: Vec<Played> = chunk.context.into_iter().map(|t| Played { ms: chunk.start, record: Record::Text(t), context: true, frame: None, markers: vec![], hits: vec![] }).collect();
        out.extend(head.advance(replay, ms));
        Ok((head, out))
    }
    /// Every record after the newest played up to `to` (into the chunks after this one as they come).
    pub fn advance(&mut self, replay: &Replay, to: u32) -> Vec<Played> {
        let mut out = vec![];
        loop {
            while let Some(t) = self.records.get(self.next).filter(|t| t.ms <= to) {
                let t = t.clone();
                self.next += 1;
                self.ms = t.ms;
                let (mut frame, mut markers, mut hits) = (None, vec![], vec![]);
                if let Record::Frame(bytes) = &t.record {
                    match self.decoder.frame(bytes) {
                        Ok((_, f)) => {
                            markers = self.tally.observe(t.ms, &f);
                            hits = self.tally.hits.clone();
                            frame = Some(f);
                        }
                        Err(e) => {
                            self.stopped = Some(format!("a frame at {} ms does not read: {}", t.ms, e.0));
                            continue;
                        }
                    }
                }
                out.push(Played { ms: t.ms, record: t.record, context: false, frame, markers, hits });
            }
            if self.next < self.records.len() || self.chunk + 1 >= replay.chunks.len() || replay.chunks[self.chunk + 1].start > to {
                return out;
            }
            match replay.chunk(self.chunk + 1) {
                Ok(chunk) => {
                    self.chunk += 1;
                    (self.records, self.next) = (chunk.records, 0);
                }
                Err(e) => {
                    self.stopped = Some(e.to_string());
                    return out;
                }
            }
        }
    }
    /// Whether everything has been played.
    pub fn done(&self, replay: &Replay) -> bool {
        self.next >= self.records.len() && (self.chunk + 1 >= replay.chunks.len() || self.stopped.is_some())
    }
}

// ----- What a single-player game records.

/// A stage as a frame names it.
pub fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Warmup => "Warmup",
        Stage::Wave => "Wave",
        Stage::FreeForAll => "FreeForAll",
        Stage::RoundOver => "RoundOver",
        Stage::Build => "Build",
        Stage::Fight => "Fight",
    }
}
/// A single-player round as a frame (what the server's game makes of its own, crates/server game.rs `frame`): every
/// hotdog seated, the one `human` plays (if any) a person's, the rest bots. `color` is each hotdog's ring, by id.
pub fn snapshot(f: &Feast, human: Option<u32>, color: impl Fn(u32) -> String) -> Frame {
    Frame {
        kind: "snapshot".into(),
        round: f.round,
        elapsed: f.elapsed,
        stage: stage_name(f.stage).into(),
        warmup: f.warmup,
        famine_at: f.famine_at,
        alert: f.alert.clone(),
        winner: f.winner,
        wave_spawned: f.wave_spawned,
        wave_kills: f.wave_kills,
        friendly_fire_events: f.friendly_fire_events,
        players: f.players.iter().map(|p| Seat { player: p.clone(), color: color(p.id), human: Some(p.id) == human, ..Default::default() }).collect(),
        enemies: f.enemies.clone(),
        emergences: f.emergences.clone(),
        pickups: f.pickups.clone(),
        shots: f.shots.clone(),
        projectiles: f.projectiles.clone(),
        splats: f.splats.clone(),
        pests: f.pests.clone(),
        turrets: f.turret_yaw.clone(),
        props: (0..f.props.len())
            .map(|id| Loose { id, state: f.props.state(id) as u8, p: f.props.position(id), q: f.props.rotation(id), v: f.props.velocity(id), w: f.props.angular_velocity(id) })
            .collect(),
        knocks: f.knocks.clone(),
        drapes: f.drapes.clone(),
        forts: f.fort_state(),
        rules: f.rules.text(),
        ends: f.ends,
        choice: Choice::default(),
        ..Default::default()
    }
}
/// The text messages a client is sent beside the frames, made as the server makes them (crates/server hub.rs).
pub mod say {
    use serde_json::json;
    pub fn kungfu(view: &crate::kungfu::View) -> String {
        let mut v = serde_json::to_value(view).unwrap_or_default();
        v["type"] = json!("kungfu");
        v.to_string()
    }
    pub fn ammo(stocks: &[crate::magazine::Stock]) -> String {
        json!({"type": "ammo", "hotdogs": stocks}).to_string()
    }
    pub fn griddle(states: &[crate::griddle::State]) -> String {
        json!({"type": "griddle", "griddles": states}).to_string()
    }
    pub fn cards(view: &crate::cards::View) -> String {
        json!({"type": "cards", "table": view}).to_string()
    }
    pub fn tuning(text: &str) -> String {
        json!({"type": "tuning", "text": text, "editable": false}).to_string()
    }
    pub fn map(text: &str) -> String {
        json!({"type": "map", "map": text}).to_string()
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;

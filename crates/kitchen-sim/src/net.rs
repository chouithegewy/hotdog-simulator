// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The game socket's protocol (protocol 12), shared by the server (crates/server) and the client (src/network.rs,
//! src/arena.rs), so that both ends encode, decode and step with the same code.
//!
//! **Time.** The server steps the round TICK_HZ times a second and numbers its steps (`tick`); it sends a frame every
//! TICK_HZ / SEND_HZ steps. Both rates are in its welcome.
//!
//! **Commands** (client → server, binary, `Batch`). The client samples its input once a step of the server's length
//! and numbers each sample (`Command::seq`): the stick (or WASD) as a vector in the hotdog's own frame, the yaw and
//! pitch it looks along, the stances it holds (crouch, prone), whether the trigger is held, whether it aims down its
//! sights (`button::SCOPE`, which a server that does not know it masks away), how it holds what it carries (`button::TURN`
//! and `button::TIP`, held, masked away as SCOPE is), and the edges since the last command (jump, stim, the melee strike, eat, pick up or put down, throw, a pickup, a
//! reload: `button::RELOAD`, both of TIP's bits, a tip no pose has). A batch carries the
//! newest commands not yet acknowledged (at most MAX_COMMANDS, the last of them new), the newest frame the client
//! has decoded (its ack, the baseline the server may encode against) and whether it asks for a whole frame.
//! The server queues them per hotdog and applies one a step (two while it is catching up) with kitchen-sim's own
//! movement and actions; `Command::stride` and `pace` are the sums both ends do.
//!
//! **Frames** (server → client, binary). What a frame says is first made into whole numbers (`State`: tables of
//! records of integers and strings, positions to the millimetre, angles to a ten-thousandth of a radian, times to the
//! millisecond, countdowns as the moment they run out, so that they do not change from step to step). A frame is
//! encoded against the state of a frame the client has acknowledged (its `base`): a table lists the records gone and
//! those that changed, a changed record only the fields that changed, by how much; a record that did not change costs
//! nothing. With no base (joining, a new map, a client that asked) the frame is whole. Every frame carries the
//! checksum of the state it describes, which the client checks after decoding: a mismatch makes it ask for a whole
//! frame. Things that happen once (`Events`: shots, splats, knocks) are sent once, in the next frame after them.
//! Beside the shared state, each frame tells its client the last of its commands the server has applied (`ack`)
//! and, while it plays, its own body as the server has it afterwards (`Mine`): where the client's prediction starts
//! again from.
//!
//! `Frame` is the typed form of all this (the snapshot as JSON clients knew it in protocol 11, with `tick`, `ack` and
//! `mine`), which the server also sends as JSON to a socket that asks for it (the checks' debug encoding).
use crate::actor::Actor;
use crate::drape::Drape;
use crate::emergence::{Emergence, Puddle};
use crate::feast::{Damage, Knock, Pickup, Player, Projectile, Shot, Splat, Swarm, Zombie, DROOP, GUNS};
use crate::forts::{BaconState, FortState};
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The join handshake's version: client and server must be built from the same revision.
pub const PROTOCOL: u32 = 12;
/// Steps a second the server simulates, and frames a second it sends (see the server's --tick and --send).
pub const TICK_HZ: u32 = 60;
pub const SEND_HZ: u32 = 30;
/// The first byte of a binary message: a client's batch of commands, a server's frame.
pub const BATCH: u8 = 1;
pub const FRAME: u8 = 2;
/// The most commands a batch carries.
pub const MAX_COMMANDS: usize = 8;
/// The guns a hotdog's record has always carried the rounds of in their places (weapons 0 to 6); the rounds of any
/// gun after them come at the end of its record, where a client of the six guns does not look and a server of the
/// six guns sends none (net.rs `Frame::state`).
const PLACES: usize = 7;
/// The bit after the buttons that says a command names a pickup.
const PICKED: u64 = 1 << 9;

/// A message that cannot be read: what was wrong with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Malformed(pub &'static str);

// ----- Bytes

/// Little-endian bytes, LEB128 varints and zigzag-coded signed numbers.
#[derive(Default)]
pub struct Writer {
    pub bytes: Vec<u8>,
}
impl Writer {
    pub fn new() -> Writer {
        Writer { bytes: Vec::with_capacity(256) }
    }
    pub fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    pub fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.bytes.push(byte);
                return;
            }
            self.bytes.push(byte | 0x80);
        }
    }
    pub fn zigzag(&mut self, v: i64) {
        self.varint(((v << 1) ^ (v >> 63)) as u64);
    }
    pub fn f32(&mut self, v: f32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn text(&mut self, s: &str) {
        self.varint(s.len() as u64);
        self.bytes.extend_from_slice(s.as_bytes());
    }
}
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }
    pub fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
    pub fn u8(&mut self) -> Result<u8, Malformed> {
        let v = *self.bytes.get(self.at).ok_or(Malformed("cut short"))?;
        self.at += 1;
        Ok(v)
    }
    pub fn varint(&mut self) -> Result<u64, Malformed> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(Malformed("a number too long"))
    }
    pub fn zigzag(&mut self) -> Result<i64, Malformed> {
        let v = self.varint()?;
        Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
    }
    /// A count of things that each take at least a byte: never more than there are bytes left.
    pub fn count(&mut self) -> Result<usize, Malformed> {
        let n = self.varint()?;
        if n > (self.bytes.len() - self.at) as u64 {
            return Err(Malformed("a count larger than the message"));
        }
        Ok(n as usize)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        let end = self.at.checked_add(n).filter(|end| *end <= self.bytes.len()).ok_or(Malformed("cut short"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    pub fn f32(&mut self) -> Result<f32, Malformed> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u32(&mut self) -> Result<u32, Malformed> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn text(&mut self) -> Result<String, Malformed> {
        let n = self.count()?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| Malformed("text that is not UTF-8"))
    }
}

// ----- Commands

/// The bits of `Command::buttons`. CROUCH, PRONE and FIRE are held; the rest are edges, set in the one command that
/// follows the press.
pub mod button {
    pub const CROUCH: u16 = 1;
    pub const PRONE: u16 = 2;
    /// The trigger is held (not while carrying: a carried object is thrown with THROW).
    pub const FIRE: u16 = 4;
    pub const JUMP: u16 = 8;
    pub const STIM: u16 = 16;
    pub const MELEE: u16 = 32;
    pub const EAT: u16 = 64;
    pub const GRAB: u16 = 128;
    pub const THROW: u16 = 256;
    /// Aiming down the sights, held: the right mouse button with the syrup rifle or the ketchup sniper in hand
    /// (`Feast::set_aimed`). It is the bit after PICKED, so the varint a command's buttons travel in is read as it always
    /// was: a server of protocol 12 from before it masks it away with its ALL (511) and reads everything else as
    /// before, and this end's ALL keeps it. A client from before it never sets it.
    pub const SCOPE: u16 = 1 << 10;
    /// How a carried object is held (kitchen-sim build.rs `Pose`), held as SCOPE is: its turn in eighths of a turn in
    /// the three bits above SCOPE, its tip in the two above those (`Command::pose`). Like SCOPE they come after PICKED:
    /// a server of protocol 12 from before them masks them away with its ALL and holds everything as it was picked up;
    /// a client from before them never sets them.
    pub const TURN: u16 = 7 << 11;
    pub const TIP: u16 = 3 << 14;
    /// How far up the buttons the pose's bits start.
    pub const POSE_SHIFT: u16 = 11;
    /// R: reload the gun in hand (kitchen-sim magazine.rs), an edge. The buttons have no bit to spare, so it is both of
    /// TIP's bits at once: a tip no pose has (build.rs `Pose::TIPS` is 3, and `Pose::from_bits` reads this one as
    /// standing), sent only with the hands free, when no pose is sent. Ask `Command::reloads`, not `has`: `has` is
    /// true for either bit. A server from before it reads it as a carried object held standing, and carries nothing.
    pub const RELOAD: u16 = TIP;
    /// Every bit there is.
    pub const ALL: u16 = 511 | SCOPE | TURN | TIP;
    pub const EDGES: u16 = JUMP | STIM | MELEE | EAT | GRAB | THROW;
}
/// One step of a player's input (see the module's documentation): quantised as it is sent, so that the client's
/// prediction works with exactly what the server will.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Command {
    pub seq: u32,
    /// The stick: [right, back] in the hotdog's own frame, each -127..127 for -1..1 (WASD is a unit vector).
    pub stick: [i8; 2],
    /// The yaw, a 65536th of a turn, and the pitch, -32767..32767 for -1.5..1.5 radians.
    pub yaw: u16,
    pub pitch: i16,
    pub buttons: u16,
    /// The gun in hand, 1 to 7, or 0 to leave it. (A server of the six guns refuses a batch naming the seventh, which a
    /// client only names once its server has given it the gun.)
    pub weapon: u8,
    /// A pickup to collect (its number), or None.
    pub pickup: Option<u16>,
}
const PITCH: f32 = 1.5;
impl Command {
    /// A command from unquantised input.
    pub fn new(seq: u32, stick: Vec2, yaw: f32, pitch: f32, buttons: u16, weapon: u8, pickup: Option<usize>) -> Command {
        let stick = stick.clamp_length_max(1.);
        let axis = |v: f32| (v * 127.).round().clamp(-127., 127.) as i8;
        let turn = (yaw.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 65536.).round() as u32 % 65536;
        Command {
            seq,
            stick: [axis(stick.x), axis(stick.y)],
            yaw: turn as u16,
            pitch: (pitch.clamp(-PITCH, PITCH) / PITCH * 32767.).round() as i16,
            buttons: buttons & button::ALL,
            weapon: weapon.min(GUNS as u8 - 1),
            pickup: pickup.map(|p| p.min(u16::MAX as usize) as u16),
        }
    }
    pub fn has(&self, bit: u16) -> bool {
        self.buttons & bit != 0
    }
    /// Whether the command asks for a reload (`button::RELOAD`: both of TIP's bits).
    pub fn reloads(&self) -> bool {
        self.buttons & button::RELOAD == button::RELOAD
    }
    /// How the hotdog holds what it carries (`button::TURN` and `button::TIP`).
    pub fn pose(&self) -> crate::feast::build::Pose {
        crate::feast::build::Pose::from_bits(((self.buttons & (button::TURN | button::TIP)) >> button::POSE_SHIFT) as u8)
    }
    /// The buttons that say a pose (to be or'd into a command's buttons).
    pub fn posed(pose: crate::feast::build::Pose) -> u16 {
        (pose.bits() as u16) << button::POSE_SHIFT
    }
    /// The stick, at most 1 long.
    pub fn stick(&self) -> Vec2 {
        Vec2::new(self.stick[0] as f32 / 127., self.stick[1] as f32 / 127.).clamp_length_max(1.)
    }
    pub fn yaw(&self) -> f32 {
        self.yaw as f32 / 65536. * std::f32::consts::TAU
    }
    pub fn pitch(&self) -> f32 {
        self.pitch as f32 / 32767. * PITCH
    }
    /// The way the hotdog aims: its yaw, then its pitch.
    pub fn direction(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw()) * Quat::from_rotation_x(self.pitch()) * Vec3::NEG_Z
    }
    /// How far this command walks the hotdog in `dt` seconds at `pace` metres a second, as [x, z] on the counter.
    pub fn stride(&self, pace: f32, dt: f32) -> Vec2 {
        let s = self.stick();
        let v = Quat::from_rotation_y(self.yaw()) * Vec3::new(s.x, 0., s.y) * pace;
        Vec2::new(v.x, v.z) * dt
    }
}
/// A person's walking pace in metres a second: the game's walk, slower crouching (65%) and prone (40%), faster on
/// stim (150%), and slower carrying something heavy (`burden`, see Feast::burden).
pub fn pace(prone: bool, crouch: bool, stim: bool, burden: f32) -> f32 {
    let stance = if prone {
        crate::actor::PRONE_PACE
    } else if crouch {
        crate::actor::CROUCH_PACE
    } else {
        1.
    } as f32;
    crate::actor::WALK as f32 * stance * if stim { crate::actor::STIM_PACE as f32 } else { 1. } * burden
}
/// What a client sends: its newest commands, the newest frame it has decoded, whether it asks for a whole frame, and
/// its own clock (milliseconds since it connected).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    pub ack: u32,
    pub resync: bool,
    pub time: u32,
    pub commands: Vec<Command>,
}
impl Batch {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(BATCH);
        w.varint(self.ack as u64);
        w.u8(self.resync as u8);
        w.varint(self.time as u64);
        let commands = &self.commands[self.commands.len().saturating_sub(MAX_COMMANDS)..];
        w.varint(commands.first().map_or(0, |c| c.seq) as u64);
        w.u8(commands.len() as u8);
        for c in commands {
            w.u8(c.stick[0] as u8);
            w.u8(c.stick[1] as u8);
            w.bytes.extend_from_slice(&c.yaw.to_le_bytes());
            w.bytes.extend_from_slice(&c.pitch.to_le_bytes());
            w.varint(c.buttons as u64 | if c.pickup.is_some() { PICKED } else { 0 });
            w.u8(c.weapon);
            if let Some(p) = c.pickup {
                w.varint(p as u64);
            }
        }
        w.bytes
    }
    /// Reads a batch; its commands are numbered one after another from the first's.
    pub fn decode(bytes: &[u8]) -> Result<Batch, Malformed> {
        let mut r = Reader::new(bytes);
        if r.u8()? != BATCH {
            return Err(Malformed("not a batch of commands"));
        }
        let ack = u32::try_from(r.varint()?).map_err(|_| Malformed("an ack out of range"))?;
        let resync = r.u8()? != 0;
        let time = r.varint()? as u32;
        let first = u32::try_from(r.varint()?).map_err(|_| Malformed("a command number out of range"))?;
        let n = r.u8()? as usize;
        if n > MAX_COMMANDS {
            return Err(Malformed("too many commands"));
        }
        let mut commands = Vec::with_capacity(n);
        for k in 0..n {
            let stick = [r.u8()? as i8, r.u8()? as i8];
            let b = r.take(4)?;
            let (yaw, pitch) = (u16::from_le_bytes([b[0], b[1]]), i16::from_le_bytes([b[2], b[3]]));
            let buttons = r.varint()?;
            let weapon = r.u8()?;
            let pickup = if buttons & PICKED != 0 { Some(r.varint()?.min(u16::MAX as u64) as u16) } else { None };
            if stick.contains(&i8::MIN) || weapon as usize >= GUNS || pitch == i16::MIN {
                return Err(Malformed("a command out of range"));
            }
            let seq = first.checked_add(k as u32).ok_or(Malformed("a command number out of range"))?;
            commands.push(Command { seq, stick, yaw, pitch, buttons: buttons as u16 & button::ALL, weapon, pickup });
        }
        if !r.done() {
            return Err(Malformed("bytes after the commands"));
        }
        Ok(Batch { ack, resync, time, commands })
    }
}

// ----- The state, in whole numbers

/// One thing in a table: its number, and what is known of it as whole numbers and strings (each table has its own
/// layout, `Frame::state`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rec {
    pub id: u32,
    pub ints: Vec<i64>,
    pub texts: Vec<String>,
}
/// The tables of a state, in this order.
pub const GLOBAL: usize = 0;
pub const PLAYERS: usize = 1;
pub const ENEMIES: usize = 2;
pub const EMERGENCES: usize = 3;
pub const PICKUPS: usize = 4;
pub const ROUNDS: usize = 5;
pub const PESTS: usize = 6;
pub const TURRETS: usize = 7;
pub const PROPS: usize = 8;
pub const DRAPES: usize = 9;
pub const SIDES: usize = 10;
pub const TABLES: usize = 11;
/// What a frame says, in whole numbers: each table's records in order of their numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub tables: [Vec<Rec>; TABLES],
}
impl State {
    /// FNV-1a over every table, record, number and string, in order: the same state has the same checksum on every
    /// machine.
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                h = (h ^ *b as u32).wrapping_mul(0x0100_0193);
            }
        };
        for (t, table) in self.tables.iter().enumerate() {
            eat(&[t as u8, 0xff]);
            for rec in table {
                eat(&rec.id.to_le_bytes());
                eat(&(rec.ints.len() as u32).to_le_bytes());
                for v in &rec.ints {
                    eat(&v.to_le_bytes());
                }
                eat(&(rec.texts.len() as u32).to_le_bytes());
                for s in &rec.texts {
                    eat(&(s.len() as u32).to_le_bytes());
                    eat(s.as_bytes());
                }
            }
        }
        h
    }
    /// About how many bytes it takes in memory.
    pub fn weight(&self) -> usize {
        self.tables.iter().flatten().map(|r| 48 + r.ints.len() * 8 + r.texts.iter().map(|s| 24 + s.len()).sum::<usize>()).sum()
    }
}
static EMPTY: Rec = Rec { id: 0, ints: Vec::new(), texts: Vec::new() };
/// A record against what the client already has of it (`base`; a new record against nothing): only what changed.
fn write_rec(w: &mut Writer, base: Option<&Rec>, rec: &Rec) {
    let base = base.unwrap_or(&EMPTY);
    let reshaped = base.ints.len() != rec.ints.len();
    let ints = reshaped || base.ints != rec.ints;
    let texts = base.texts != rec.texts;
    w.u8(ints as u8 | (reshaped as u8) << 1 | (texts as u8) << 2);
    if reshaped {
        w.varint(rec.ints.len() as u64);
        for v in &rec.ints {
            w.zigzag(*v);
        }
    } else if ints {
        let changed: Vec<usize> = (0..rec.ints.len()).filter(|i| rec.ints[*i] != base.ints[*i]).collect();
        w.varint(changed.len() as u64);
        let mut last = 0;
        for (k, i) in changed.into_iter().enumerate() {
            w.varint((i - if k == 0 { 0 } else { last + 1 }) as u64);
            w.zigzag(rec.ints[i].wrapping_sub(base.ints[i]));
            last = i;
        }
    }
    if texts {
        w.varint(rec.texts.len() as u64);
        let mask: u64 = (0..rec.texts.len().min(64)).filter(|i| base.texts.get(*i) != Some(&rec.texts[*i])).fold(0, |m, i| m | 1 << i);
        w.varint(mask);
        for (i, s) in rec.texts.iter().enumerate().take(64) {
            if mask & 1 << i != 0 {
                w.text(s);
            }
        }
    }
}
fn read_rec(r: &mut Reader, id: u32, base: Option<&Rec>) -> Result<Rec, Malformed> {
    let mut rec = base.cloned().unwrap_or_default();
    rec.id = id;
    let head = r.u8()?;
    if head & 2 != 0 {
        let n = r.count()?;
        rec.ints = (0..n).map(|_| r.zigzag()).collect::<Result<_, _>>()?;
    } else if head & 1 != 0 {
        let n = r.count()?;
        let mut i = 0usize;
        for k in 0..n {
            i = if k == 0 { 0 } else { i + 1 } + r.varint()? as usize;
            let slot = rec.ints.get_mut(i).ok_or(Malformed("a field beyond the record"))?;
            *slot = slot.wrapping_add(r.zigzag()?);
        }
    }
    if head & 4 != 0 {
        let n = r.count()?;
        rec.texts.resize(n, String::new());
        let mask = r.varint()?;
        for (i, s) in rec.texts.iter_mut().enumerate().take(64) {
            if mask & 1 << i != 0 {
                *s = r.text()?;
            }
        }
    }
    Ok(rec)
}
/// A table against the client's: the numbers of the records gone, then the records new or changed.
fn write_table(w: &mut Writer, base: &[Rec], now: &[Rec]) {
    let gone: Vec<u32> = base.iter().filter(|b| now.binary_search_by_key(&b.id, |r| r.id).is_err()).map(|b| b.id).collect();
    let changed: Vec<(Option<&Rec>, &Rec)> = now
        .iter()
        .map(|rec| (base.binary_search_by_key(&rec.id, |b| b.id).ok().map(|i| &base[i]), rec))
        .filter(|(base, rec)| *base != Some(*rec))
        .collect();
    w.varint(gone.len() as u64);
    let mut last = 0;
    for id in gone {
        w.varint((id - last) as u64);
        last = id;
    }
    w.varint(changed.len() as u64);
    let mut last = 0;
    for (base, rec) in changed {
        w.varint((rec.id - last) as u64);
        last = rec.id;
        write_rec(w, base, rec);
    }
}
fn read_table(r: &mut Reader, base: &[Rec]) -> Result<Vec<Rec>, Malformed> {
    let n = r.count()?;
    let mut gone = Vec::with_capacity(n);
    let mut last = 0u32;
    for _ in 0..n {
        last = last.checked_add(r.varint()? as u32).ok_or(Malformed("a number out of range"))?;
        gone.push(last);
    }
    let mut table: Vec<Rec> = base.iter().filter(|b| !gone.contains(&b.id)).cloned().collect();
    let n = r.count()?;
    let mut last = 0u32;
    for k in 0..n {
        let step = r.varint()?;
        if k > 0 && step == 0 {
            return Err(Malformed("a record twice"));
        }
        last = last.checked_add(u32::try_from(step).map_err(|_| Malformed("a number out of range"))?).ok_or(Malformed("a number out of range"))?;
        match table.binary_search_by_key(&last, |rec| rec.id) {
            Ok(i) => {
                let rec = read_rec(r, last, Some(&table[i]))?;
                table[i] = rec;
            }
            Err(i) => {
                let rec = read_rec(r, last, None)?;
                table.insert(i, rec);
            }
        }
    }
    Ok(table)
}
/// Every table of `state` against `base` (or whole), behind a mask of the tables that changed.
pub fn write_state(w: &mut Writer, base: Option<&State>, state: &State) {
    let empty = State::default();
    let base = base.unwrap_or(&empty);
    let mask: u64 = (0..TABLES).filter(|t| base.tables[*t] != state.tables[*t]).fold(0, |m, t| m | 1 << t);
    w.varint(mask);
    for t in 0..TABLES {
        if mask & 1 << t != 0 {
            write_table(w, &base.tables[t], &state.tables[t]);
        }
    }
}
pub fn read_state(r: &mut Reader, base: Option<&State>) -> Result<State, Malformed> {
    let empty = State::default();
    let base = base.unwrap_or(&empty);
    let mask = r.varint()?;
    let mut state = base.clone();
    for t in 0..TABLES {
        if mask & 1 << t != 0 {
            state.tables[t] = read_table(r, &base.tables[t])?;
        }
    }
    Ok(state)
}

// ----- The frame

/// The client's own body as the server has it after the last of its commands applied: what a step of it depends
/// on beyond the command (actor.rs), so that the client can step it again from there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mine {
    pub p: [f32; 3],
    /// The vertical speed (the rest of a body's velocity is not carried from step to step), and what is left of a
    /// throw.
    pub vy: f32,
    pub knock: [f32; 2],
    pub grounded: bool,
    pub launched: bool,
    pub perch: Option<[f32; 2]>,
    pub support: Option<f32>,
    pub pin: Option<[f32; 4]>,
}
impl Mine {
    pub fn of(body: &Actor) -> Mine {
        let f = |v: f64| v as f32;
        Mine {
            p: body.p.map(f),
            vy: f(body.v[1]),
            knock: body.knock.map(f),
            grounded: body.grounded,
            launched: body.launched,
            perch: body.perch.map(|v| v.map(f)),
            support: body.support.map(f),
            pin: body.pin.map(|v| v.map(f)),
        }
    }
    /// `body` (its id, size and stance) where the server has it.
    pub fn onto(&self, body: &mut Actor) {
        let d = |v: f32| v as f64;
        body.p = self.p.map(d);
        body.v = [0., d(self.vy), 0.];
        body.knock = self.knock.map(d);
        (body.grounded, body.launched, body.fallen) = (self.grounded, self.launched, false);
        body.perch = self.perch.map(|v| v.map(d));
        body.support = self.support.map(d);
        body.pin = self.pin.map(|v| v.map(d));
        body.jump = 0.;
    }
    fn write(&self, w: &mut Writer) {
        let flags = self.grounded as u8 | (self.launched as u8) << 1 | (self.perch.is_some() as u8) << 2 | (self.support.is_some() as u8) << 3 | (self.pin.is_some() as u8) << 4;
        w.u8(flags);
        for v in self.p.iter().chain([&self.vy]).chain(&self.knock) {
            w.f32(*v);
        }
        for v in self.perch.iter().flatten().chain(&self.support).chain(self.pin.iter().flatten()) {
            w.f32(*v);
        }
    }
    fn read(r: &mut Reader) -> Result<Mine, Malformed> {
        let flags = r.u8()?;
        let mut f = || r.f32();
        let (p, vy, knock) = ([f()?, f()?, f()?], f()?, [f()?, f()?]);
        let perch = if flags & 4 != 0 { Some([f()?, f()?]) } else { None };
        let support = if flags & 8 != 0 { Some(f()?) } else { None };
        let pin = if flags & 16 != 0 { Some([f()?, f()?, f()?, f()?]) } else { None };
        Ok(Mine { p, vy, knock, grounded: flags & 1 != 0, launched: flags & 2 != 0, perch, support, pin })
    }
}
/// A name and a colour a player chose (the server's lobby checks them).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub color: String,
}
/// A hotdog as a frame carries it: the round's record of it and its seat's.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Seat {
    #[serde(flatten)]
    pub player: Player,
    pub color: String,
    pub human: bool,
    pub waiting: bool,
    pub talk: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Forts: the team its player asked for (Some(None): left to the server), once the lobby has said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wish: Option<Option<u8>>,
}
/// The next round's map as the lobby has it ({name, title, mode, theme}, or Random: name `random`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub name: String,
    pub title: String,
    pub mode: String,
    pub theme: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub random: bool,
}
/// A loose object: its state (props.rs `State` as a number), the place of its origin, its rotation as a quaternion,
/// and its linear and angular velocity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Loose {
    pub id: usize,
    pub state: u8,
    pub p: [f64; 3],
    pub q: [f64; 4],
    pub v: [f64; 3],
    pub w: [f64; 3],
}
/// What happened once since the last frame.
#[derive(Clone, Default)]
pub struct Events {
    pub shots: Vec<Shot>,
    pub splats: Vec<Splat>,
    pub knocks: Vec<Knock>,
}
/// A frame (see the module's documentation): the protocol-11 snapshot with the step it was taken at, the last command
/// of its client's applied, and that client's own body.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    #[serde(rename = "type")]
    pub kind: String,
    pub tick: u32,
    pub ack: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mine: Option<Mine>,
    pub countdown: Option<f32>,
    pub opening: f32,
    pub opening_for: f32,
    pub round: u32,
    pub elapsed: f32,
    pub stage: String,
    pub warmup: f32,
    pub famine_at: f32,
    pub alert: String,
    pub winner: Option<u32>,
    pub wave_spawned: u32,
    pub wave_kills: u32,
    pub friendly_fire_events: u32,
    pub players: Vec<Seat>,
    pub enemies: Vec<Zombie>,
    pub emergences: Vec<Emergence>,
    pub pickups: Vec<Pickup>,
    pub shots: Vec<Shot>,
    pub projectiles: Vec<Projectile>,
    pub splats: Vec<Splat>,
    pub pests: Vec<Swarm>,
    pub turrets: Vec<f32>,
    pub props: Vec<Loose>,
    pub knocks: Vec<Knock>,
    pub drapes: Vec<Drape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forts: Option<FortState>,
    pub leader: u32,
    pub choice: Choice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sides: Option<BTreeMap<String, u8>>,
    /// What the game is played by beyond its map (rules.rs `Rules::text`: Free For All, the sandbox; empty for the
    /// usual game), and Free For All's end on the round's clock (0: none).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rules: String,
    pub ends: f32,
}
/// The stages as a frame numbers them.
pub const STAGES: [&str; 8] = ["Lobby", "Opening", "Warmup", "Wave", "FreeForAll", "RoundOver", "Build", "Fight"];

// Units: positions to the millimetre (drapes to a tenth of one, loose objects and their rotations to 1e-5, as finely
// as the client compares them), angles to 1e-4 radians, speeds to a millimetre a second, times to the millisecond,
// HP to a hundredth, unit vectors to 1e-4.
const MM: f64 = 1e3;
const FINE: f64 = 1e4;
const PROP: f64 = 1e5;
const ANGLE: f64 = 1e4;
const MS: f64 = 1e3;
const HUNDREDTHS: f64 = 1e2;
const UNIT: f64 = 1e4;
fn q(v: f64, unit: f64) -> i64 {
    let x = (v * unit).round();
    if x.is_finite() { x.clamp(-1e15, 1e15) as i64 } else { 0 }
}
fn q3(v: [f32; 3], unit: f64) -> [i64; 3] {
    v.map(|x| q(x as f64, unit))
}
fn d(v: i64, unit: f64) -> f32 {
    (v as f64 / unit) as f32
}
fn d64(v: i64, unit: f64) -> f64 {
    v as f64 / unit
}
/// Reads `n` numbers off a list, 0 for any it has not got.
struct Ints<'a>(&'a [i64], usize);
impl Ints<'_> {
    fn next(&mut self) -> i64 {
        let v = self.0.get(self.1).copied().unwrap_or(0);
        self.1 += 1;
        v
    }
    fn f(&mut self, unit: f64) -> f32 {
        d(self.next(), unit)
    }
    fn f3(&mut self, unit: f64) -> [f32; 3] {
        [self.f(unit), self.f(unit), self.f(unit)]
    }
    fn count(&mut self) -> usize {
        (self.next().max(0) as usize).min(self.0.len())
    }
}
fn text(rec: &Rec, i: usize) -> String {
    rec.texts.get(i).cloned().unwrap_or_default()
}
fn rec(id: u32, ints: Vec<i64>, texts: Vec<String>) -> Rec {
    Rec { id, ints, texts }
}
/// A countdown as the moment it runs out (-1 for none), and back.
fn until(elapsed: f32, left: f32) -> i64 {
    if left > 0. { q((elapsed + left) as f64, MS) } else { -1 }
}
fn left(elapsed: f32, until: i64) -> f32 {
    if until < 0 { 0. } else { (d(until, MS) - elapsed).max(0.) }
}
/// A clock counting up as the moment it started (-1 for one at 0), and back.
fn since(elapsed: f32, count: f32) -> i64 {
    if count > 0. { q((elapsed - count) as f64, MS) } else { -1 }
}
fn counted(elapsed: f32, since: i64) -> f32 {
    if since < 0 { 0. } else { (elapsed - d(since, MS)).max(0.) }
}

impl Frame {
    /// What the frame says, in whole numbers (see the module's documentation; the layout of each table is here and
    /// in `from_state`). Not in it: the step, the ack, the client's own body and the events.
    pub fn state(&self) -> State {
        let e = self.elapsed;
        let mut s = State::default();
        let stage = STAGES.iter().position(|n| *n == self.stage).unwrap_or(0) as i64;
        let fort = self.forts.as_ref();
        let none = FortState::default();
        let f = fort.unwrap_or(&none);
        let fight = self.stage == "Fight";
        let mut ints = vec![
            self.round as i64,
            stage,
            q(self.warmup as f64, MS),
            q(self.famine_at as f64, MS),
            self.winner.map_or(0, |w| w as i64 + 1),
            self.wave_spawned as i64,
            self.wave_kills as i64,
            self.friendly_fire_events as i64,
            self.countdown.map_or(-2, |c| q((e + c) as f64, MS)),
            since(e, self.opening),
            q(self.opening_for as f64, MS),
            self.leader as i64,
            self.choice.random as i64,
            fort.is_some() as i64,
        ];
        ints.extend([f.scores[0] as i64, f.scores[1] as i64, f.rounds as i64, f.match_no as i64, q(f.build as f64, MS), q(f.ends as f64, MS), f.field as i64]);
        ints.extend([f.bacon.state as i64, f.bacon.holder as i64]);
        ints.extend(q3(f.bacon.p, MM));
        ints.extend([since(e, f.bacon.idle), if fight { until(e, f.stale) } else { q(f.stale as f64, MS) }, f.alarms as i64, f.winner.map_or(0, |w| w as i64 + 2), f.champion.map_or(0, |c| c as i64 + 1)]);
        // (Free For All's end, and the rules, after everything protocol 12 had first: a client that knows nothing of
        // them reads what it did.)
        ints.push(q(self.ends as f64, MS));
        let texts = vec![self.alert.clone(), self.choice.name.clone(), self.choice.title.clone(), self.choice.mode.clone(), self.choice.theme.clone(), f.reason.clone(), self.rules.clone()];
        s.tables[GLOBAL].push(rec(0, ints, texts));
        for seat in &self.players {
            let p = &seat.player;
            // (Aiming down the sights is the last bit: a client from before it reads every other bit where it was.)
            let flags = [p.alive, p.bot, p.consumed, p.infected, p.prone, p.crouch, p.grounded, p.launched, p.fallen, p.ready, seat.human, seat.waiting, seat.profile.is_some(), p.aimed]
                .iter()
                .enumerate()
                .fold(0i64, |m, (i, b)| m | (*b as i64) << i);
            let mut ints = Vec::with_capacity(40);
            ints.extend(q3(p.p, MM));
            ints.extend([q(p.yaw as f64, ANGLE), q(p.pitch as f64, ANGLE)]);
            ints.extend(q3(p.v, MM));
            ints.extend([q(p.hp as f64, HUNDREDTHS), flags, p.weapon as i64, p.gun as i64, p.carry as i64 + 1, p.score as i64, p.kills as i64, p.meals as i64, p.swings as i64, p.team as i64]);
            // The six guns' rounds here, as protocol 12 has always had them; any gun after them at the end.
            ints.extend(p.ammo[..PLACES].iter().map(|a| *a as i64));
            ints.push(p.owned.iter().enumerate().fold(0i64, |m, (i, o)| m | (*o as i64) << i));
            ints.extend([until(e, p.stim), until(e, p.stim_wait), q(seat.talk as f64, HUNDREDTHS), seat.wish.map_or(0, |w| w.map_or(1, |t| t as i64 + 2))]);
            let l = &p.last_damage;
            ints.extend([l.kind as i64, l.attacker as i64]);
            ints.extend(q3(l.p, MM));
            ints.extend(q3(l.d, UNIT));
            ints.extend([q(l.time as f64, MS), q(l.amount as f64, HUNDREDTHS)]);
            ints.extend(p.ammo[PLACES..].iter().map(|a| *a as i64));
            // Free For All and the sandbox (rules.rs): deaths, when it is back, its spawn protection.
            ints.extend([p.deaths as i64, until(e, p.respawn), until(e, p.shield)]);
            let profile = seat.profile.clone().unwrap_or_default();
            s.tables[PLAYERS].push(rec(p.id, ints, vec![p.name.clone(), seat.color.clone(), profile.name, profile.color]));
        }
        for z in &self.enemies {
            let mut ints = q3(z.p, MM).to_vec();
            ints.extend([q(z.yaw as f64, ANGLE), q(z.hp as f64, HUNDREDTHS), q(z.born as f64, MS), until(e, z.forming)]);
            s.tables[ENEMIES].push(rec(z.id, ints, vec![]));
        }
        for m in &self.emergences {
            let mut ints = vec![q(m.started as f64, MS), m.total as i64, m.begun as i64, q(m.ends as f64, MS), m.sources.len() as i64];
            ints.extend(m.sources.iter().map(|s| *s as i64));
            ints.push(m.puddles.len() as i64);
            for p in &m.puddles {
                ints.push(p.spawn as i64);
                ints.extend(q3(p.p, MM));
                ints.extend([p.source as i64, q(p.length as f64, MM), q(p.arrive as f64, MS), q(p.until as f64, MS), p.count as i64, p.path.len() as i64]);
                ints.extend(p.path.iter().flat_map(|[x, z]| [q(*x as f64, MM), q(*z as f64, MM)]));
            }
            ints.push(m.schedule.len() as i64);
            ints.extend(m.schedule.iter().flat_map(|(at, i)| [q(*at as f64, MS), *i as i64]));
            s.tables[EMERGENCES].push(rec(m.id, ints, vec![]));
        }
        for p in &self.pickups {
            let mut ints = q3(p.p, MM).to_vec();
            ints.extend([p.weapon as i64, p.ammo as i64, q(p.ready as f64, MS), q(p.respawn as f64, MS)]);
            s.tables[PICKUPS].push(rec(p.id as u32, ints, vec![]));
        }
        for r in &self.projectiles {
            let mut ints = vec![r.owner as i64, r.weapon as i64];
            // A round that falls is where its arc puts it (feast::arc), so it is sent as it left the nozzle, and when:
            // the same numbers all its flight, which cost nothing after the first frame.
            if DROOP.get(r.weapon).is_some_and(|fall| *fall > 0.) {
                ints.extend(q3(r.from, MM));
                ints.extend(q3(r.launch, MM));
                ints.extend([0, r.hostile as i64, q(r.born as f64, MS)]);
            } else {
                ints.extend(q3(r.p, MM));
                ints.extend(q3(r.v, MM));
                ints.extend([q(r.travelled as f64, MM), r.hostile as i64]);
            }
            s.tables[ROUNDS].push(rec(r.id, ints, vec![]));
        }
        for w in &self.pests {
            let mut ints = q3(w.p, MM).to_vec();
            ints.extend([q(w.hp as f64, HUNDREDTHS), q(w.born as f64, MS), since(e, w.empty)]);
            s.tables[PESTS].push(rec(w.id, ints, vec![]));
        }
        for (i, yaw) in self.turrets.iter().enumerate() {
            s.tables[TURRETS].push(rec(i as u32, vec![q(*yaw as f64, ANGLE)], vec![]));
        }
        for l in &self.props {
            let mut ints = vec![l.state as i64];
            ints.extend(l.p.map(|v| q(v, PROP)));
            ints.extend(l.q.map(|v| q(v, PROP)));
            ints.extend(l.v.map(|v| q(v, MM)));
            ints.extend(l.w.map(|v| q(v, MM)));
            s.tables[PROPS].push(rec(l.id as u32, ints, vec![]));
        }
        for t in &self.drapes {
            let mut ints = vec![t.napkin as i64];
            ints.extend(t.rect.map(|v| q(v, FINE)));
            ints.extend([q(t.top, FINE), t.props.len() as i64]);
            ints.extend(t.props.iter().map(|p| *p as i64));
            s.tables[DRAPES].push(rec(t.id, ints, vec![]));
        }
        for (id, side) in self.sides.iter().flatten() {
            if let Ok(id) = id.parse::<u32>() {
                s.tables[SIDES].push(rec(id, vec![*side as i64], vec![]));
            }
        }
        for table in &mut s.tables {
            table.sort_by_key(|r| r.id);
            table.dedup_by_key(|r| r.id);
        }
        s
    }
    /// The frame a state says, at the round's clock `elapsed` (from the frame's header).
    pub fn from_state(s: &State, elapsed: f32) -> Frame {
        let e = elapsed;
        let mut frame = Frame { kind: "snapshot".into(), elapsed, ..Default::default() };
        if let Some(g) = s.tables[GLOBAL].first() {
            let mut i = Ints(&g.ints, 0);
            frame.round = i.next() as u32;
            frame.stage = STAGES.get(i.next() as usize).unwrap_or(&STAGES[0]).to_string();
            frame.warmup = i.f(MS);
            frame.famine_at = i.f(MS);
            frame.winner = match i.next() {
                0 => None,
                w => Some((w - 1) as u32),
            };
            frame.wave_spawned = i.next() as u32;
            frame.wave_kills = i.next() as u32;
            frame.friendly_fire_events = i.next() as u32;
            frame.countdown = match i.next() {
                -2 => None,
                c => Some(d(c, MS) - e),
            };
            frame.opening = counted(e, i.next());
            frame.opening_for = i.f(MS);
            frame.leader = i.next() as u32;
            let random = i.next() != 0;
            let fort = i.next() != 0;
            let mut f = FortState { scores: [i.next() as u32, i.next() as u32], rounds: i.next() as u32, match_no: i.next() as u32, build: i.f(MS), ends: i.f(MS), field: i.next() != 0, ..Default::default() };
            f.bacon = BaconState { state: i.next() as u8, holder: i.next() as u32, p: i.f3(MM), idle: 0. };
            f.bacon.idle = counted(e, i.next());
            let stale = i.next();
            f.stale = if frame.stage == "Fight" { left(e, stale) } else { d(stale, MS) };
            f.alarms = i.next() as u32;
            f.winner = match i.next() {
                0 => None,
                w => Some((w - 2) as i32),
            };
            f.champion = match i.next() {
                0 => None,
                c => Some((c - 1) as u32),
            };
            f.reason = text(g, 5);
            frame.ends = i.f(MS);
            frame.rules = text(g, 6);
            frame.forts = fort.then_some(f);
            frame.alert = text(g, 0);
            frame.choice = Choice { name: text(g, 1), title: text(g, 2), mode: text(g, 3), theme: text(g, 4), random };
        }
        for r in &s.tables[PLAYERS] {
            let mut i = Ints(&r.ints, 0);
            let mut p = Player { id: r.id, name: text(r, 0), ..Default::default() };
            p.p = i.f3(MM);
            (p.yaw, p.pitch) = (i.f(ANGLE), i.f(ANGLE));
            p.v = i.f3(MM);
            p.hp = i.f(HUNDREDTHS);
            let flags = i.next();
            let on = |bit: usize| flags & 1 << bit != 0;
            (p.alive, p.bot, p.consumed, p.infected, p.prone, p.crouch, p.grounded, p.launched, p.fallen, p.ready) = (on(0), on(1), on(2), on(3), on(4), on(5), on(6), on(7), on(8), on(9));
            p.aimed = on(13);
            p.weapon = i.next() as usize;
            p.gun = i.next() as usize;
            p.carry = (i.next() - 1) as i32;
            (p.score, p.kills, p.meals, p.swings, p.team) = (i.next() as u32, i.next() as u32, i.next() as u32, i.next() as u32, i.next() as u8);
            for a in &mut p.ammo[..PLACES] {
                *a = i.next() as i32;
            }
            let owned = i.next();
            p.owned = std::array::from_fn(|k| owned & 1 << k != 0);
            p.stim = left(e, i.next());
            p.stim_wait = left(e, i.next());
            let talk = i.f(HUNDREDTHS);
            let wish = match i.next() {
                0 => None,
                1 => Some(None),
                t => Some(Some((t - 2) as u8)),
            };
            p.last_damage = Damage { kind: i.next() as u8, attacker: i.next() as u32, p: i.f3(MM), d: i.f3(UNIT), time: i.f(MS), amount: i.f(HUNDREDTHS) };
            // (None from a server of the six guns.)
            for a in &mut p.ammo[PLACES..] {
                *a = i.next() as i32;
            }
            (p.deaths, p.respawn, p.shield) = (i.next() as u32, left(e, i.next()), left(e, i.next()));
            let profile = on(12).then(|| Profile { name: text(r, 2), color: text(r, 3) });
            frame.players.push(Seat { player: p, color: text(r, 1), human: on(10), waiting: on(11), talk, profile, wish });
        }
        for r in &s.tables[ENEMIES] {
            let mut i = Ints(&r.ints, 0);
            let p = i.f3(MM);
            let (yaw, hp, born) = (i.f(ANGLE), i.f(HUNDREDTHS), i.f(MS));
            frame.enemies.push(Zombie { id: r.id, p, yaw, hp, born, forming: left(e, i.next()) });
        }
        for r in &s.tables[EMERGENCES] {
            let mut i = Ints(&r.ints, 0);
            let mut m = Emergence { id: r.id, started: i.f(MS), total: i.next() as u32, begun: i.next() as u32, ends: i.f(MS), ..Default::default() };
            let n = i.count();
            m.sources = (0..n).map(|_| i.next() as usize).collect();
            let n = i.count();
            for _ in 0..n {
                let mut p = Puddle { spawn: i.next() as usize, p: i.f3(MM), ..Default::default() };
                (p.source, p.length, p.arrive, p.until, p.count) = (i.next() as i32, i.f(MM), i.f(MS), i.f(MS), i.next() as u32);
                let k = i.count();
                p.path = (0..k).map(|_| [i.f(MM), i.f(MM)]).collect();
                m.puddles.push(p);
            }
            let n = i.count();
            m.schedule = (0..n).map(|_| (i.f(MS), i.next() as usize)).collect();
            frame.emergences.push(m);
        }
        for r in &s.tables[PICKUPS] {
            let mut i = Ints(&r.ints, 0);
            let p = i.f3(MM);
            frame.pickups.push(Pickup { id: r.id as usize, p, weapon: i.next() as usize, ammo: i.next() as i32, ready: i.f(MS), respawn: i.f(MS) });
        }
        for r in &s.tables[ROUNDS] {
            let mut i = Ints(&r.ints, 0);
            let (owner, weapon) = (i.next() as u32, i.next() as usize);
            let (p, v) = (i.f3(MM), i.f3(MM));
            let mut round = Projectile { id: r.id, owner, weapon, p, v, travelled: i.f(MM), hostile: i.next() != 0, ..Default::default() };
            let fall = DROOP.get(weapon).copied().unwrap_or(0.);
            if fall > 0. && r.ints.len() > 10 {
                (round.born, round.from, round.launch) = (i.f(MS), p, v);
                round.age = (e - round.born).max(0.);
                let (at, now) = crate::feast::arc(Vec3::from_array(p), Vec3::from_array(v), fall, round.age);
                (round.p, round.v, round.travelled) = (at.to_array(), now.to_array(), Vec3::from_array(v).length() * round.age);
            }
            frame.projectiles.push(round);
        }
        for r in &s.tables[PESTS] {
            let mut i = Ints(&r.ints, 0);
            let p = i.f3(MM);
            frame.pests.push(Swarm { id: r.id, p, hp: i.f(HUNDREDTHS), born: i.f(MS), empty: counted(e, i.next()) });
        }
        frame.turrets = s.tables[TURRETS].iter().map(|r| d(r.ints.first().copied().unwrap_or(0), ANGLE)).collect();
        for r in &s.tables[PROPS] {
            let mut i = Ints(&r.ints, 0);
            let state = i.next() as u8;
            let mut f = |n: usize, unit: f64| -> Vec<f64> { (0..n).map(|_| d64(i.next(), unit)).collect() };
            let (p, q, v, w) = (f(3, PROP), f(4, PROP), f(3, MM), f(3, MM));
            frame.props.push(Loose { id: r.id as usize, state, p: [p[0], p[1], p[2]], q: [q[0], q[1], q[2], q[3]], v: [v[0], v[1], v[2]], w: [w[0], w[1], w[2]] });
        }
        for r in &s.tables[DRAPES] {
            let mut i = Ints(&r.ints, 0);
            let napkin = i.next() as usize;
            let rect = [d64(i.next(), FINE), d64(i.next(), FINE), d64(i.next(), FINE), d64(i.next(), FINE)];
            let top = d64(i.next(), FINE);
            let n = i.count();
            frame.drapes.push(Drape { id: r.id, napkin, props: (0..n).map(|_| i.next() as usize).collect(), rect, top });
        }
        if !s.tables[SIDES].is_empty() {
            frame.sides = Some(s.tables[SIDES].iter().map(|r| (r.id.to_string(), r.ints.first().copied().unwrap_or(0) as u8)).collect());
        }
        frame
    }
}

// ----- Frames on the wire

/// What a binary frame says beside its state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Header {
    pub tick: u32,
    /// The step of the frame it is encoded against (one the client acknowledged), or 0 for a whole frame.
    pub base: u32,
    pub ack: u32,
    pub checksum: u32,
    pub elapsed: f32,
    pub mine: Option<Mine>,
}
fn write_events(w: &mut Writer, events: &Events) {
    let p = |w: &mut Writer, v: [f32; 3]| {
        for x in q3(v, MM) {
            w.zigzag(x);
        }
    };
    w.varint(events.shots.len() as u64);
    for s in &events.shots {
        w.varint(s.id as u64);
        w.varint(s.owner as u64);
        p(w, s.a);
        p(w, s.b);
        w.zigzag(q(s.time as f64, MS));
        w.varint(s.weapon as u64);
        w.varint(s.projectile as u64);
    }
    w.varint(events.splats.len() as u64);
    for s in &events.splats {
        w.varint(s.id as u64);
        w.varint(s.projectile as u64);
        w.varint(s.weapon as u64);
        p(w, s.p);
        w.zigzag(q(s.time as f64, MS));
    }
    w.varint(events.knocks.len() as u64);
    for k in &events.knocks {
        w.varint(k.id as u64);
        w.varint(k.prop as u64);
        w.zigzag(k.other as i64);
        p(w, k.p);
        w.zigzag(q(k.speed as f64, MM));
        w.zigzag(q(k.time as f64, MS));
    }
}
fn read_events(r: &mut Reader) -> Result<Events, Malformed> {
    let mut events = Events::default();
    let p = |r: &mut Reader| -> Result<[f32; 3], Malformed> { Ok([d(r.zigzag()?, MM), d(r.zigzag()?, MM), d(r.zigzag()?, MM)]) };
    for _ in 0..r.count()? {
        let (id, owner) = (r.varint()? as u32, r.varint()? as u32);
        let (a, b) = (p(r)?, p(r)?);
        events.shots.push(Shot { id, owner, a, b, time: d(r.zigzag()?, MS), weapon: r.varint()? as usize, projectile: r.varint()? as u32 });
    }
    for _ in 0..r.count()? {
        let (id, projectile, weapon) = (r.varint()? as u32, r.varint()? as u32, r.varint()? as usize);
        events.splats.push(Splat { id, projectile, weapon, p: p(r)?, time: d(r.zigzag()?, MS) });
    }
    for _ in 0..r.count()? {
        let (id, prop, other) = (r.varint()? as u32, r.varint()? as u32, r.zigzag()? as i32);
        events.knocks.push(Knock { id, prop, other, p: p(r)?, speed: d(r.zigzag()?, MM), time: d(r.zigzag()?, MS) });
    }
    Ok(events)
}
/// A frame as bytes: its header, `state` against `base` (the state of the frame `header.base` names), the events.
pub fn encode_frame(header: &Header, base: Option<&State>, state: &State, events: &Events) -> Vec<u8> {
    let mut w = Writer::new();
    w.u8(FRAME);
    w.varint(header.tick as u64);
    w.varint(if base.is_some() { header.base as u64 } else { 0 });
    w.varint(header.ack as u64);
    w.u32(header.checksum);
    w.f32(header.elapsed);
    match &header.mine {
        Some(mine) => {
            w.u8(1);
            mine.write(&mut w);
        }
        None => w.u8(0),
    }
    write_state(&mut w, base, state);
    write_events(&mut w, events);
    w.bytes
}
/// The step a frame was taken at and the step of the frame it is encoded against (0: whole), read first so that the
/// client can find that frame's state.
pub fn peek_frame(bytes: &[u8]) -> Result<(u32, u32), Malformed> {
    let mut r = Reader::new(bytes);
    if r.u8()? != FRAME {
        return Err(Malformed("not a frame"));
    }
    Ok((r.varint()? as u32, r.varint()? as u32))
}
/// Reads a frame against `base` (the state of the frame its header names, None for a whole frame).
pub fn decode_frame(bytes: &[u8], base: Option<&State>) -> Result<(Header, State, Events), Malformed> {
    let mut r = Reader::new(bytes);
    if r.u8()? != FRAME {
        return Err(Malformed("not a frame"));
    }
    let tick = r.varint()? as u32;
    let base_tick = r.varint()? as u32;
    if base_tick != 0 && base.is_none() {
        return Err(Malformed("a frame against a state the client has not got"));
    }
    let ack = r.varint()? as u32;
    let checksum = r.u32()?;
    let elapsed = r.f32()?;
    let mine = if r.u8()? != 0 { Some(Mine::read(&mut r)?) } else { None };
    let state = read_state(&mut r, if base_tick == 0 { None } else { base })?;
    let events = read_events(&mut r)?;
    if !r.done() {
        return Err(Malformed("bytes after the frame"));
    }
    Ok((Header { tick, base: base_tick, ack, checksum, elapsed, mine }, state, events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feast::{Feast, FLOOR};
    use crate::map::Map;

    fn frame_of(s: &Feast) -> Frame {
        Frame {
            kind: "snapshot".into(),
            round: s.round,
            elapsed: s.elapsed,
            stage: format!("{:?}", s.stage),
            warmup: s.warmup,
            famine_at: s.famine_at,
            alert: s.alert.clone(),
            winner: s.winner,
            wave_spawned: s.wave_spawned,
            wave_kills: s.wave_kills,
            players: s.players.iter().map(|p| Seat { player: p.clone(), color: "red".into(), human: p.id == 1, talk: 0.25, ..Default::default() }).collect(),
            enemies: s.enemies.clone(),
            emergences: s.emergences.clone(),
            pickups: s.pickups.clone(),
            projectiles: s.projectiles.clone(),
            pests: s.pests.clone(),
            turrets: s.turret_yaw.clone(),
            props: (0..s.props.len())
                .map(|id| Loose { id, state: s.props.state(id) as u8, p: s.props.position(id), q: s.props.rotation(id), v: s.props.velocity(id), w: s.props.angular_velocity(id) })
                .collect(),
            drapes: s.drapes.clone(),
            forts: s.fort_state(),
            choice: Choice { name: "counter-siege".into(), title: "Counter Siege".into(), mode: "siege".into(), theme: "kitchen".into(), random: false },
            ..Default::default()
        }
    }
    /// The state of a frame made back into a frame is the same state: nothing is lost that the quantisation keeps.
    fn same_state(frame: &Frame) -> State {
        let state = frame.state();
        let back = Frame::from_state(&state, frame.elapsed);
        assert_eq!(back.state(), state, "a state survives being made into a frame and back");
        state
    }
    #[test]
    fn a_command_is_quantised_once_and_both_ends_walk_the_same_stride_from_it() {
        let c = Command::new(7, Vec2::new(0.6, -0.8), -1.0, 0.31, button::FIRE | button::JUMP, 2, Some(3));
        let batch = Batch { ack: 400, resync: true, time: 12345, commands: vec![c, Command { seq: 8, ..c }] };
        let bytes = batch.encode();
        assert!(bytes.len() < 30, "two commands in {} bytes", bytes.len());
        assert_eq!(Batch::decode(&bytes).unwrap(), batch);
        // The yaw wraps, to a 65536th of a turn; the pitch to 1.5/32767 of a radian; the stick to 1/127.
        assert!((c.yaw() - (std::f32::consts::TAU - 1.0)).abs() < 1e-4);
        assert!((c.pitch() - 0.31).abs() < 5e-5);
        assert!((c.stick() - Vec2::new(0.6, -0.8)).length() < 0.01 && c.stick().length() <= 1.);
        // A full push forward at the walking pace, facing -z, walks WALK (0.57 m/s) along -z.
        let ahead = Command::new(1, Vec2::new(0., -1.), 0., 0., 0, 1, None);
        let s = ahead.stride(pace(false, false, false, 1.), 0.5);
        assert!((s - Vec2::new(0., -0.5 * crate::actor::WALK as f32)).length() < 1e-6, "{s}");
        assert!((pace(false, true, true, 0.8) - crate::actor::WALK as f32 * 0.65 * 1.5 * 0.8).abs() < 1e-6);
        // Cut short, or more commands than a batch may carry: refused.
        assert!(Batch::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(Batch::decode(&[BATCH, 0, 0, 0, 1, 9]).is_err(), "more commands than a batch may carry");
    }
    /// A batch's commands read as a server of protocol 12 from before `button::SCOPE` read them: its buttons masked
    /// with the nine bits it knew (511), and the pickup that PICKED says follows.
    fn read_as_before(bytes: &[u8]) -> Vec<(u16, u8, Option<u16>, [i8; 2])> {
        let mut r = Reader::new(bytes);
        assert_eq!(r.u8().unwrap(), BATCH);
        let (_ack, _resync, _time, _first) = (r.varint().unwrap(), r.u8().unwrap(), r.varint().unwrap(), r.varint().unwrap());
        let n = r.u8().unwrap() as usize;
        let mut out = vec![];
        for _ in 0..n {
            let stick = [r.u8().unwrap() as i8, r.u8().unwrap() as i8];
            r.take(4).unwrap();
            let buttons = r.varint().unwrap();
            let weapon = r.u8().unwrap();
            let pickup = if buttons & PICKED != 0 { Some(r.varint().unwrap() as u16) } else { None };
            out.push((buttons as u16 & 511, weapon, pickup, stick));
        }
        assert!(r.done(), "nothing left over");
        out
    }
    #[test]
    fn aiming_down_the_sights_travels_in_a_command_and_a_server_from_before_it_reads_the_rest_as_it_always_did() {
        assert_eq!(button::SCOPE as u64, PICKED << 1, "the bit after the one that marks a pickup");
        assert_eq!(button::ALL & 511, 511);
        assert_eq!(button::EDGES & button::SCOPE, 0, "held, not an edge");
        let aimed = Command::new(7, Vec2::new(0.6, -0.8), -1.0, 0.31, button::SCOPE | button::FIRE | button::CROUCH, 6, Some(3));
        assert!(aimed.has(button::SCOPE) && aimed.has(button::FIRE));
        let plain = Command { seq: 8, buttons: button::SCOPE, pickup: None, ..aimed };
        let batch = Batch { ack: 400, resync: false, time: 12345, commands: vec![aimed, plain] };
        let bytes = batch.encode();
        assert_eq!(Batch::decode(&bytes).unwrap(), batch, "SCOPE round-trips, with the pickup after it");
        // Read as an older server reads it: no SCOPE, and everything else, the pickup included, where it was.
        let before = read_as_before(&bytes);
        assert_eq!(before, vec![(button::FIRE | button::CROUCH, 6, Some(3), aimed.stick), (0, 6, None, aimed.stick)]);
        // And without SCOPE the bytes are what they always were.
        let unaimed = Batch { commands: vec![Command { buttons: button::FIRE | button::CROUCH, ..aimed }], ..batch.clone() };
        assert_eq!(read_as_before(&unaimed.encode())[0], before[0]);
        assert!(unaimed.encode().len() < bytes.len());
    }
    /// How a carried object is held travels in a command's buttons, above SCOPE: every turn and tip round-trips, and a
    /// server from before reads the rest as it always did (the pickup after them included).
    #[test]
    fn the_carried_objects_turn_and_tip_travel_in_a_command_and_a_server_from_before_reads_the_rest_as_it_always_did() {
        use crate::feast::build::Pose;
        assert_eq!(button::TURN & (511 | button::SCOPE), 0);
        assert_eq!(button::TIP & (511 | button::SCOPE | button::TURN), 0);
        assert_eq!(button::EDGES & (button::TURN | button::TIP), 0, "held, not edges");
        let mut commands = vec![];
        for turn in 0..Pose::TURNS {
            for tip in 0..Pose::TIPS {
                let pose = Pose { turn, tip };
                assert_eq!(Pose::from_bits(pose.bits()), pose);
                let c = Command::new(9 + commands.len() as u32, Vec2::new(0.3, 0.), 0.5, 0.1, button::GRAB | button::CROUCH | Command::posed(pose), 0, Some(2));
                assert_eq!(c.pose(), pose);
                commands.push(c);
            }
        }
        // A tip of 3 (no such tip) is read as none.
        assert_eq!(Pose::from_bits(3 << 3), Pose::default());
        let batch = Batch { ack: 7, resync: false, time: 99, commands: commands[..MAX_COMMANDS].to_vec() };
        let bytes = batch.encode();
        assert_eq!(Batch::decode(&bytes).unwrap(), batch);
        for (k, (buttons, weapon, pickup, _)) in read_as_before(&bytes).into_iter().enumerate() {
            assert_eq!((buttons, weapon, pickup), (button::GRAB | button::CROUCH, 0, Some(2)), "command {k}");
        }
    }
    #[test]
    fn a_hotdog_aiming_down_its_sights_is_the_last_of_its_flags_and_a_client_from_before_reads_the_others_unchanged() {
        let mut s = Feast::new(false);
        s.players[1].bot = false;
        let mut frame = frame_of(&s);
        let flags = |frame: &Frame| frame.state().tables[PLAYERS][1].ints[9];
        let was = flags(&frame);
        frame.players[1].player.aimed = true;
        let now = flags(&frame);
        assert_eq!((now, now & !(1 << 13)), (was | 1 << 13, was), "bit 13, and nothing else moves");
        assert_eq!(frame.state().tables[PLAYERS][1].ints.len(), frame_of(&s).state().tables[PLAYERS][1].ints.len(), "no new field");
        let back = Frame::from_state(&same_state(&frame), frame.elapsed);
        assert!(back.players[1].player.aimed && !back.players[0].player.aimed);
        assert_eq!(back.players[1].player.alive, frame.players[1].player.alive);
    }
    #[test]
    fn quantisation_keeps_positions_to_half_a_millimetre_angles_and_times_and_whole_numbers_exactly() {
        let mut s = Feast::with_map(Map::bundled(), true);
        s.tick(14.);
        let frame = frame_of(&s);
        let back = Frame::from_state(&same_state(&frame), frame.elapsed);
        for (a, b) in frame.players.iter().zip(&back.players) {
            let (a, b) = (&a.player, &b.player);
            for k in 0..3 {
                assert!((a.p[k] - b.p[k]).abs() <= 0.0005 + 1e-6, "{} {:?} {:?}", a.id, a.p, b.p);
                assert!((a.v[k] - b.v[k]).abs() <= 0.0005 + 1e-6);
            }
            assert!((a.yaw - b.yaw).abs() <= 5e-5 + 1e-6 && (a.pitch - b.pitch).abs() <= 5e-5 + 1e-6);
            assert!((a.hp - b.hp).abs() <= 0.005 + 1e-5 && (a.stim - b.stim).abs() <= 0.001);
            assert_eq!((a.ammo, a.owned, a.weapon, a.alive, a.grounded, &a.name), (b.ammo, b.owned, b.weapon, b.alive, b.grounded, &b.name));
        }
        for (a, b) in frame.props.iter().zip(&back.props) {
            assert!((0..3).all(|k| (a.p[k] - b.p[k]).abs() <= 0.000005 + 1e-12), "props to a two-hundredth of a millimetre");
            assert!((0..4).all(|k| (a.q[k] - b.q[k]).abs() <= 0.000005 + 1e-12));
        }
        assert_eq!(back.pickups.len(), frame.pickups.len());
        assert_eq!(back.choice, frame.choice);
        assert_eq!(back.stage, frame.stage);
        // A position far out (the kitchen floor beyond a counter's edge, a body thrown high) is kept too.
        let mut far = frame.clone();
        far.players[0].player.p = [-4.4996, -0.0004, 4.9994];
        let p = Frame::from_state(&far.state(), far.elapsed).players[0].player.p;
        assert_eq!(p, [-4.5, -0., 4.999]);
    }
    #[test]
    fn a_delta_against_any_acknowledged_state_rebuilds_the_state_exactly_and_costs_little_when_little_moved() {
        let mut s = Feast::with_map(Map::bundled(), true);
        s.tick(13.);
        let mut states = vec![];
        for _ in 0..12 {
            s.tick(1. / 30.);
            states.push(frame_of(&s).state());
        }
        let whole = encode_frame(&Header { tick: 12, checksum: states[11].checksum(), ..Default::default() }, None, &states[11], &Events::default());
        // Against the last frame, an older one (an ack that was dropped or is late) and none at all.
        for base in [10usize, 5, 0] {
            let header = Header { tick: 12, base: base as u32 + 1, ack: 3, checksum: states[11].checksum(), elapsed: s.elapsed, mine: None };
            let bytes = encode_frame(&header, Some(&states[base]), &states[11], &Events::default());
            assert_eq!(peek_frame(&bytes).unwrap(), (12, base as u32 + 1));
            let (h, state, _) = decode_frame(&bytes, Some(&states[base])).unwrap();
            assert_eq!(state, states[11]);
            assert_eq!((h.ack, h.checksum), (3, state.checksum()));
            assert!(bytes.len() < whole.len(), "a delta ({} bytes against {base}) is smaller than the whole ({})", bytes.len(), whole.len());
        }
        // Nothing changed: a few bytes.
        let bytes = encode_frame(&Header { tick: 13, base: 12, ..Default::default() }, Some(&states[11]), &states[11], &Events::default());
        assert!(bytes.len() <= 22, "an unchanged frame is {} bytes", bytes.len());
        // Against a state the client has not got: refused, not misread.
        assert!(decode_frame(&bytes, None).is_err());
        // Against the wrong state: it decodes to something else, which the checksum gives away.
        let delta = encode_frame(&Header { tick: 12, base: 11, checksum: states[11].checksum(), ..Default::default() }, Some(&states[10]), &states[11], &Events::default());
        if let Ok((h, wrong, _)) = decode_frame(&delta, Some(&states[2])) {
            assert_ne!(wrong.checksum(), h.checksum);
        }
    }
    #[test]
    fn records_come_and_go_texts_change_and_garbage_is_refused_without_a_panic() {
        let mut a = State::default();
        a.tables[ENEMIES] = (1..=5).map(|id| rec(id, vec![id as i64, -3, 7], vec![])).collect();
        a.tables[PLAYERS] = vec![rec(1, vec![0; 40], vec!["Ann".into(), "red".into()])];
        let mut b = a.clone();
        b.tables[ENEMIES].retain(|r| r.id != 2 && r.id != 5);
        b.tables[ENEMIES].push(rec(9, vec![1, 2, 3, 4], vec![]));
        b.tables[ENEMIES][0].ints[2] = 8;
        b.tables[PLAYERS][0].texts[1] = "blue".into();
        b.tables[PLAYERS][0].ints[39] = -1;
        let mut w = Writer::new();
        write_state(&mut w, Some(&a), &b);
        let decoded = read_state(&mut Reader::new(&w.bytes), Some(&a)).unwrap();
        assert_eq!(decoded, b);
        // Truncations and noise: an error every time, never a panic.
        for cut in 0..w.bytes.len() {
            let _ = read_state(&mut Reader::new(&w.bytes[..cut]), Some(&a));
        }
        let mut seed = 7u32;
        for _ in 0..2000 {
            let bytes: Vec<u8> = (0..(seed % 40) as usize)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (seed >> 16) as u8
                })
                .collect();
            let _ = decode_frame(&bytes, Some(&a));
            let _ = Batch::decode(&bytes);
        }
    }
    #[test]
    fn events_and_the_own_body_travel_as_they_are() {
        let events = Events {
            shots: vec![Shot { id: 4, owner: 2, a: [0.1, FLOOR + 0.135, 2.4], b: [0.1, FLOOR + 0.135, 2.43], time: 3.25, weapon: 6, projectile: 9 }],
            splats: vec![Splat { id: 3, projectile: 9, weapon: 6, p: [0.4, FLOOR, 2.0], time: 3.5 }],
            knocks: vec![Knock { id: 1, prop: 0, other: -2, p: [1.2, FLOOR, 2.75], speed: 0.62, time: 3.0 }],
        };
        let mine = Mine { p: [0.123_456_7, FLOOR as f32, 2.4], vy: 0.53, knock: [0.2, -0.1], grounded: false, launched: true, perch: None, support: Some(0.95), pin: Some([1., 0., 0.1, 2.4]) };
        let header = Header { tick: 99, base: 0, ack: 77, checksum: 5, elapsed: 3.6, mine: Some(mine) };
        let bytes = encode_frame(&header, None, &State::default(), &events);
        let (h, _, e) = decode_frame(&bytes, None).unwrap();
        assert_eq!(h, header);
        assert_eq!((e.shots[0].id, e.shots[0].weapon, e.shots[0].projectile, e.splats[0].projectile, e.knocks[0].other), (4, 6, 9, 9, -2));
        assert!((e.knocks[0].speed - 0.62).abs() < 1e-3 && (e.splats[0].time - 3.5).abs() < 1e-6);
        // The body comes back exactly.
        let mut body = Actor::hotdog(1, [0.; 3]);
        mine.onto(&mut body);
        assert_eq!(Mine::of(&body), mine);
    }
    /// The bundled Forts maps' stock (the kitchen's pieces, some in piles): the whole frame a joining
    /// client is sent stays under 64 KB, and in the build, with the bots at work, a frame against the one before it
    /// (a 30th of a second, as the server sends them) carries next to nothing for the piles at rest.
    #[test]
    fn a_forts_frame_with_all_the_stock_out_is_small_whole_and_smaller_as_a_delta() {
        for name in ["pillow-fort", "jungle-fort", "moonlit-fort", "the-whole-kitchen-forts"] {
            let path = format!("{}/../../assets/maps/{name}.json", env!("CARGO_MANIFEST_DIR"));
            let mut s = Feast::with_map(Map::parse(&std::fs::read_to_string(&path).unwrap()).unwrap(), false);
            s.set_build(600.);
            for _ in 0..60 {
                s.tick(1. / 30.);
            }
            let (mut whole, mut deltas, mut last) = (0, vec![], same_state(&frame_of(&s)));
            for step in 0..90 {
                s.tick(1. / 30.);
                let state = frame_of(&s).state();
                let header = Header { tick: step + 2, base: step + 1, checksum: state.checksum(), elapsed: s.elapsed, ..Default::default() };
                deltas.push(encode_frame(&header, Some(&last), &state, &Events::default()).len());
                whole = whole.max(encode_frame(&Header { tick: step + 2, checksum: state.checksum(), ..Default::default() }, None, &state, &Events::default()).len());
                last = state;
            }
            deltas.sort();
            let props = s.props.len();
            println!("{name}: {props} loose objects; whole frame {whole} bytes; deltas median {} bytes, largest {}", deltas[45], deltas[89]);
            assert!(whole < 64 * 1024, "{name}: a whole frame of {whole} bytes");
            assert!(deltas[45] < 1500 && deltas[89] < 6000, "{name}: deltas {deltas:?}");
        }
    }
}

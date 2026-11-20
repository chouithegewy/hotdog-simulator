// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Kung Fu's sounds: played from the fight as the client has it (`Game::kungfu`), offline and online alike, every frame
//! after the round. Two sets of files: the recordings the project owner supplied, cut into one-shots in
//! `assets/audio/kungfu/` by `tools/build_kungfu_user_sounds.py`, and the thirteen sounds `tools/build_kungfu_sounds.py`
//! synthesised (`assets/audio/fu-*.wav`), kept where no recording fits.
//!
//! * **Moves starting** (a fighter's move changed): a light whoosh for a light strike, a kick or a dodge, a heavy one (the
//!   synthesised one, or the swing of a recorded punch) for a heavy strike, a jump attack, a sweep or a special, and a
//!   **kiai** (a shout) on a heavy strike.
//! * **The events** (`View::events`, each taken once by its id), as `sounds` has them: a hit (a recorded punch or thump,
//!   chosen at random so a run of them does not repeat; heavier for a big one), a blow blocked (a dull recorded thud), a
//!   parry's clack, a guard's crack with a rattle of steel, a grab, a throw landing (a thump and a heavy hit) or a knockdown
//!   (a thump and a grunt), a strike ringing off a Toad's iron body, a special (its shout), and a knockout: the wok gong,
//!   which swings as well (dojo.rs), and a scream.
//! * **The round**: a sting as it begins, and one as it ends (a victory for this player's win, another otherwise).
//! * **This player's own**: the chime when its chi fills, and a short sting when its style changes.
//! * **The dojo's air**: a quiet loop of bamboo woods, for as long as the round lasts on a Kung Fu map.
//!
//! The shouts (kiai, special, scream, grunt) are a fighter's voice, and eight fighters in a ring would be a wall of them,
//! so a voice is limited (`Heard::may_shout`): a fighter shouts at most once in `VOICE_GAP` seconds, a fighter further
//! than `VOICE_RANGE` metres from this player is not heard to, and at most `VOICE_CROWD.0` voices start in any
//! `VOICE_CROWD.1` seconds (this player's own is always heard, and a knockout's scream is never dropped). Which of a
//! sound's takes is played is the client's own luck (the simulation is not asked), never the take just played, with a
//! slight random pitch so that even one take is not the same twice.
//!
//! Each sound is quieter the further its fighter is from this player (`near`), and all of them are at modest volumes, set
//! as KF4 set the synthesised ones: under the guns. (The files were checked by number, not by ear.)
use crate::arena::Game;
use bevy::{
    audio::{AudioSinkPlayback, Volume},
    prelude::*,
};
use kitchen_sim::kungfu::{self, EventKind, Kind, Move};
use std::collections::{HashMap, HashSet};

/// One recording (or synthesised file) and how fast it is played: a speed other than 1 is the same take higher or lower,
/// so that a few takes make more variety.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Take {
    pub file: &'static str,
    pub speed: f32,
}
/// A take at its own pitch, and at another (macros, so that a list of them is a constant a function may return).
macro_rules! take {
    ($file:expr) => {
        Take { file: $file, speed: 1. }
    };
}
macro_rules! at {
    ($file:expr, $speed:expr) => {
        Take { file: $file, speed: $speed }
    };
}

/// The sounds, each with the takes one of which is played.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    // Synthesised (tools/build_kungfu_sounds.py), where no recording fits.
    Whoosh,
    Parry,
    GuardBreak,
    Grab,
    Throw,
    Gong,
    Chi,
    Clang,
    // Recorded (tools/build_kungfu_user_sounds.py).
    WhooshHeavy,
    Hit,
    HitHeavy,
    Block,
    Rattle,
    Grunt,
    Kiai,
    Special,
    Scream,
    Style,
    Start,
    Win,
    End,
    Ambience,
}
impl Sound {
    pub const ALL: [Sound; 22] = [
        Sound::Whoosh,
        Sound::Parry,
        Sound::GuardBreak,
        Sound::Grab,
        Sound::Throw,
        Sound::Gong,
        Sound::Chi,
        Sound::Clang,
        Sound::WhooshHeavy,
        Sound::Hit,
        Sound::HitHeavy,
        Sound::Block,
        Sound::Rattle,
        Sound::Grunt,
        Sound::Kiai,
        Sound::Special,
        Sound::Scream,
        Sound::Style,
        Sound::Start,
        Sound::Win,
        Sound::End,
        Sound::Ambience,
    ];
    /// The takes. Several: one is picked at random each time (never the one before).
    pub fn takes(self) -> &'static [Take] {
        match self {
            Sound::Whoosh => &[take!("audio/fu-whoosh.wav")],
            Sound::Parry => &[take!("audio/fu-parry.wav")],
            Sound::GuardBreak => &[take!("audio/fu-guardbreak.wav")],
            Sound::Grab => &[take!("audio/fu-grab.wav")],
            Sound::Throw => &[take!("audio/fu-throw.wav")],
            Sound::Gong => &[take!("audio/fu-gong.wav")],
            Sound::Chi => &[take!("audio/fu-chi.wav")],
            Sound::Clang => &[take!("audio/fu-clang.wav")],
            // The synthesised heavy whoosh, and the rising air of a recorded punch's swing.
            Sound::WhooshHeavy => &[take!("audio/fu-whoosh-heavy.wav"), take!("audio/kungfu/swing.wav")],
            // The recorded punch, the heavy one raised to a lighter pitch, and two of the body thumps.
            Sound::Hit => &[
                take!("audio/kungfu/hit-punch.wav"),
                at!("audio/kungfu/hit-punch.wav", 1.12),
                at!("audio/kungfu/hit-heavy.wav", 1.22),
                take!("audio/kungfu/thud-1.wav"),
                take!("audio/kungfu/thud-3.wav"),
            ],
            // The heavy punch, and the others lowered.
            Sound::HitHeavy => &[
                at!("audio/kungfu/hit-heavy.wav", 0.92),
                take!("audio/kungfu/hit-heavy.wav"),
                at!("audio/kungfu/hit-punch.wav", 0.82),
                at!("audio/kungfu/thud-4.wav", 0.9),
            ],
            // The softer, longer body thumps: a blow taken on the arms.
            Sound::Block => &[
                take!("audio/kungfu/thud-5.wav"),
                take!("audio/kungfu/thud-6.wav"),
                take!("audio/kungfu/thud-7.wav"),
                take!("audio/kungfu/thud-8.wav"),
                at!("audio/kungfu/thud-2.wav", 0.9),
            ],
            Sound::Rattle => &[take!("audio/kungfu/rattle.wav")],
            Sound::Grunt => &[take!("audio/kungfu/grunt.wav"), at!("audio/kungfu/grunt.wav", 0.93), at!("audio/kungfu/grunt.wav", 1.07)],
            Sound::Kiai => &[take!("audio/kungfu/kiai-1.wav"), take!("audio/kungfu/kiai-2.wav"), take!("audio/kungfu/kiai-3.wav"), take!("audio/kungfu/kiai-4.wav")],
            Sound::Special => &[take!("audio/kungfu/special.wav")],
            Sound::Scream => &[
                take!("audio/kungfu/scream-1.wav"),
                take!("audio/kungfu/scream-2.wav"),
                take!("audio/kungfu/scream-3.wav"),
                take!("audio/kungfu/scream-4.wav"),
                take!("audio/kungfu/scream-5.wav"),
            ],
            Sound::Style => &[take!("audio/kungfu/sting-style.wav")],
            Sound::Start => &[take!("audio/kungfu/sting-start.wav")],
            Sound::Win => &[take!("audio/kungfu/sting-win.wav")],
            Sound::End => &[take!("audio/kungfu/sting-end.wav")],
            Sound::Ambience => &[take!("audio/kungfu/ambience-dojo.wav")],
        }
    }
    /// How loud it is played, at the listener.
    pub fn volume(self) -> f32 {
        match self {
            Sound::Whoosh => 0.45,
            Sound::WhooshHeavy => 0.5,
            Sound::Hit => 0.6,
            Sound::HitHeavy => 0.65,
            Sound::Block => 0.55,
            Sound::Parry => 0.6,
            Sound::GuardBreak => 0.65,
            Sound::Rattle => 0.4,
            Sound::Grab => 0.5,
            Sound::Throw => 0.6,
            Sound::Grunt => 0.45,
            Sound::Kiai => 0.45,
            Sound::Special => 0.5,
            Sound::Scream => 0.5,
            Sound::Gong => 0.55,
            Sound::Chi => 0.45,
            Sound::Style => 0.45,
            Sound::Clang => 0.55,
            Sound::Start => 0.5,
            Sound::Win => 0.5,
            Sound::End => 0.45,
            Sound::Ambience => 0.5,
        }
    }
    /// A fighter's voice: the shouts, which `Heard::may_shout` limits.
    pub fn is_voice(self) -> bool {
        matches!(self, Sound::Kiai | Sound::Special | Sound::Scream | Sound::Grunt)
    }
    /// Played a little higher or lower each time (the impacts and the voices; not the bells, stings and the loop).
    fn varies(self) -> bool {
        matches!(self, Sound::Hit | Sound::HitHeavy | Sound::Block | Sound::Kiai | Sound::Special | Sound::Scream | Sound::Grunt)
    }
}
/// A blow this many HP or more is a heavy hit.
pub const HEAVY: f32 = 12.;
/// How far the pitch of a varying sound strays each time (a share either way).
pub const PITCH: f32 = 0.04;
/// A fighter's voice: seconds between its shouts, how far off it is still heard (metres) and how many voices may begin
/// in how many seconds, whoever they are.
pub const VOICE_GAP: f32 = 1.2;
pub const VOICE_RANGE: f32 = 3.5;
pub const VOICE_CROWD: (usize, f32) = (3, 1.0);
/// A round's clock this young (seconds) when it is first seen, or begins again, is a start.
pub const START_WITHIN: f32 = 4.;

/// How loud a sound `far` metres off is, of its full volume: whole within half a metre, a fifth at four metres and
/// beyond (the counter is a few metres long: everything on it is heard).
pub fn near(far: f32) -> f32 {
    (1. - (far - 0.5).max(0.) / 4.4).clamp(0.2, 1.)
}

/// What a move starting sounds like, if anything.
pub fn swing(mv: Move) -> Option<Sound> {
    match mv.data().kind {
        Kind::Light | Kind::Kick => Some(Sound::Whoosh),
        Kind::Heavy | Kind::Jump | Kind::Sweep | Kind::Special => Some(Sound::WhooshHeavy),
        Kind::Dodge => Some(Sound::Whoosh),
        _ => None,
    }
}
/// The shout a move starting brings with it, if any: a kiai on a heavy strike. (A special's shout is its event's.)
pub fn shout(mv: Move) -> Option<Sound> {
    (mv.data().kind == Kind::Heavy).then_some(Sound::Kiai)
}
/// What an event sounds like: the sounds layered (none for a strike that passed through a dodge: the dodge was heard
/// as it began).
pub fn sounds(e: &kungfu::Event) -> Vec<Sound> {
    let Some(kind) = e.kind else { return vec![] };
    match kind {
        EventKind::Hit if e.mv == Move::Grab as u8 => vec![Sound::Throw, Sound::HitHeavy],
        EventKind::Hit if e.amount >= HEAVY => vec![Sound::HitHeavy],
        EventKind::Hit => vec![Sound::Hit],
        EventKind::Blocked => vec![Sound::Block],
        EventKind::Parried => vec![Sound::Parry],
        EventKind::Guardbroken => vec![Sound::GuardBreak, Sound::Rattle],
        EventKind::Grabbed => vec![Sound::Grab],
        EventKind::Knockdown => vec![Sound::Throw, Sound::Grunt],
        EventKind::Ko => vec![Sound::Gong, Sound::Scream],
        EventKind::Absorbed => vec![Sound::Clang],
        EventKind::Special => vec![Sound::Special],
        EventKind::Dodged => vec![],
    }
}

/// One sound to play: which, the take chosen, and how loud.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub sound: Sound,
    pub take: Take,
    pub volume: f32,
}

/// How the round stands, for the stings that end it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Round {
    /// The round is over (its stage is RoundOver).
    pub over: bool,
    /// ...and this player won it.
    pub won: bool,
}

/// What has been heard so far.
#[derive(Resource, Default)]
pub struct Heard {
    handles: HashMap<&'static str, Handle<AudioSource>>,
    /// The last event taken, each fighter's move (and when it began), this player's chi full and its style.
    told: u32,
    moves: HashMap<u32, (u8, u32)>,
    full: bool,
    style: Option<u8>,
    /// The round's clock at the last look (None: nothing seen yet), and whether the round was over.
    clock: Option<f32>,
    over: bool,
    /// When each fighter last shouted, and when the last few voices began (this player's clock).
    voices: HashMap<u32, f32>,
    recent: Vec<f32>,
    /// The luck of the takes, and the take each sound played last.
    luck: u64,
    last: HashMap<Sound, usize>,
    /// The dojo's air, while it plays, and the volume its sink was last set to.
    air: Option<(Entity, f32)>,
    /// What was played this frame, for the tests.
    pub played: Vec<Cue>,
}
impl Heard {
    /// A take of `sound`, at `volume`: not the one played last (when there is a choice), and a little higher or lower if it
    /// varies. `salt` stirs the luck with something of the moment, so each run of the game is not the same.
    fn cue(&mut self, sound: Sound, volume: f32, salt: u32) -> Cue {
        let takes = sound.takes();
        self.luck = self.luck.wrapping_add(0x9E37_79B9_7F4A_7C15) ^ u64::from(salt);
        let mut z = self.luck;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_D5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let mut i = (z % takes.len() as u64) as usize;
        if takes.len() > 1 && self.last.get(&sound) == Some(&i) {
            i = (i + 1 + ((z >> 32) as usize % (takes.len() - 1))) % takes.len();
        }
        self.last.insert(sound, i);
        let mut t = takes[i];
        if sound.varies() {
            t.speed *= 1. + PITCH * (((z >> 40) & 0xFFFF) as f32 / 32767.5 - 1.);
        }
        Cue { sound, take: t, volume: volume * sound.volume() }
    }
    /// Whether fighter `who`, `far` metres from this player, may shout at `now`: `ko` (a scream) always may, and a
    /// fighter's voice is `VOICE_GAP` seconds apart, in range and not beyond the crowd's limit (`mine`: this player's own,
    /// which ignores the range and the crowd). It is then counted.
    pub fn may_shout(&mut self, who: u32, now: f32, far: f32, mine: bool, ko: bool) -> bool {
        // A clock that has gone back is a new round's: nobody has shouted yet.
        if self.voices.values().chain(&self.recent).any(|t| *t > now) {
            self.voices.clear();
            self.recent.clear();
        }
        self.recent.retain(|t| now - t < VOICE_CROWD.1);
        let rested = self.voices.get(&who).is_none_or(|t| now - t >= VOICE_GAP);
        let allowed = ko || (rested && (mine || (far <= VOICE_RANGE && self.recent.len() < VOICE_CROWD.0)));
        if allowed {
            self.voices.insert(who, now);
            self.recent.push(now);
        }
        allowed
    }
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Heard>().add_systems(Update, listen.after(crate::arena::tick));
    }
}

/// What this frame's fight sounds like: the cues for everything new since `heard` last listened, which it then remembers.
/// `elapsed` is the game's clock, `round` how the round stands. Pure but for `heard`.
pub fn hear(heard: &mut Heard, view: &kungfu::View, me: u32, listener: Vec3, elapsed: f32, places: &HashMap<u32, Vec3>, round: Round) -> Vec<Cue> {
    let mut out = vec![];
    let apart = |id: u32| places.get(&id).map_or(0., |p| p.distance(listener));
    let stir = |a: u32, b: u32| a.wrapping_mul(0x45D9_F3B).wrapping_add(b);
    // The round: a start (its clock young and first seen, or run back) and an end.
    if heard.clock.is_none_or(|c| view.elapsed + 0.25 < c) && view.elapsed < START_WITHIN {
        out.push(heard.cue(Sound::Start, 1., 1));
    }
    heard.clock = Some(view.elapsed);
    if round.over && !heard.over {
        out.push(heard.cue(if round.won { Sound::Win } else { Sound::End }, 1., 2));
    }
    heard.over = round.over;
    // Moves that began since the last frame (a fighter's move, or the moment it began, changed).
    for f in &view.fighters {
        let key = (f.mv, f.started.to_bits());
        let was = heard.moves.insert(f.id, key);
        if was.is_some_and(|w| w != key) && f.t(elapsed) < 0.25 {
            let (far, salt) = (apart(f.id), stir(f.id, f.started.to_bits()));
            if let Some(s) = swing(f.mv()) {
                out.push(heard.cue(s, near(far), salt));
            }
            if let Some(s) = shout(f.mv()).filter(|_| heard.may_shout(f.id, elapsed, far, f.id == me, false)) {
                out.push(heard.cue(s, near(far), salt ^ 7));
            }
        }
    }
    heard.moves.retain(|id, _| view.fighters.iter().any(|f| f.id == *id));
    // The events not heard yet (their ids begin again from 1 each round).
    let last = view.events.iter().map(|e| e.id).max().unwrap_or(0);
    if last < heard.told {
        heard.told = 0;
    }
    let fresh: Vec<&kungfu::Event> = view.events.iter().filter(|e| e.id > heard.told).collect();
    // (A fighter knocked out also falls: it screams, and does not grunt as well.)
    let knocked_out: HashSet<u32> = fresh.iter().filter(|e| e.kind == Some(EventKind::Ko)).map(|e| e.to).collect();
    for e in fresh {
        let far = Vec3::from_array(e.p).distance(listener);
        for s in sounds(e) {
            let salt = stir(e.id, e.amount.to_bits());
            let volume = if s == Sound::Gong { 1. } else { near(far) };
            if s.is_voice() {
                // The voice is its victim's (a grunt, a scream) or its user's (a special's shout).
                let who = if s == Sound::Special { e.by } else { e.to };
                if s == Sound::Grunt && knocked_out.contains(&who) {
                    continue;
                }
                if !heard.may_shout(who, elapsed, far, who == me, s == Sound::Scream) {
                    continue;
                }
            }
            out.push(heard.cue(s, volume, salt));
        }
    }
    heard.told = heard.told.max(last);
    // This player's chi filling, and its style changing.
    if let Some(f) = view.fighter(me) {
        let full = f.chi >= kungfu::CHI_MAX - 1e-3;
        if full && !heard.full {
            out.push(heard.cue(Sound::Chi, 1., 3));
        }
        heard.full = full;
        if heard.style.is_some_and(|s| s != f.style) {
            out.push(heard.cue(Sound::Style, 1., 4));
        }
        heard.style = Some(f.style);
    }
    out
}

/// Whether a Kung Fu round is being played where the dojo's air belongs: any map of the mode (the dojo is the only one made).
fn dojo_air(g: &Game) -> bool {
    g.kungfu.is_some() && g.playing() && g.feast.level.map.mode == kungfu::MODE
}

/// Keeps the dojo's air looping while `on` (its sink follows the game's volume setting as the griddle's sizzle does:
/// settings.rs), and stops it after.
fn air(world: &mut World, heard: &mut Heard, on: bool) {
    let playing = heard.air.filter(|(e, _)| world.get_entity(*e).is_ok());
    match (on, playing) {
        (true, Some((e, was))) => {
            let global = world.get_resource::<GlobalVolume>().map_or(Volume::Linear(1.), |g| g.volume);
            let volume = Volume::Linear(Sound::Ambience.volume()) * global;
            if (volume.to_linear() - was).abs() > 1e-4 {
                if let Some(mut sink) = world.get_mut::<AudioSink>(e) {
                    sink.set_volume(volume);
                    heard.air = Some((e, volume.to_linear()));
                }
            }
        }
        (true, None) => {
            let Some(server) = world.get_resource::<AssetServer>().cloned() else { return };
            let file = Sound::Ambience.takes()[0].file;
            let handle = heard.handles.entry(file).or_insert_with(|| server.load(file)).clone();
            let global = world.get_resource::<GlobalVolume>().map_or(Volume::Linear(1.), |g| g.volume);
            let e = world.spawn((AudioPlayer::new(handle), PlaybackSettings::LOOP.with_volume(Volume::Linear(Sound::Ambience.volume())))).id();
            heard.air = Some((e, (Volume::Linear(Sound::Ambience.volume()) * global).to_linear()));
        }
        (false, Some((e, _))) => {
            world.entity_mut(e).despawn();
            heard.air = None;
        }
        (false, None) => heard.air = None,
    }
}

/// Every frame after the round: the fight's sounds, and the gong struck on a knockout.
fn listen(world: &mut World) {
    let Some(g) = world.get_resource::<Game>() else { return };
    let on = dojo_air(g);
    let Some(view) = g.kungfu.as_ref().filter(|_| g.playing()) else {
        if let Some(mut heard) = world.remove_resource::<Heard>() {
            (heard.told, heard.full, heard.style, heard.clock, heard.over) = (0, false, None, None, false);
            heard.moves.clear();
            heard.voices.clear();
            heard.recent.clear();
            air(world, &mut heard, false);
            world.insert_resource(heard);
        }
        return;
    };
    let places: HashMap<u32, Vec3> = g.roster.iter().map(|p| (p.id, Vec3::from_array(p.p))).collect();
    let (me, listener, elapsed) = (g.me(), g.position, g.elapsed);
    let round = Round { over: g.phase_name() == "RoundOver", won: g.feast.winner == Some(g.me()) };
    let view = view.clone();
    let mut heard = world.remove_resource::<Heard>().unwrap_or_default();
    let cues = hear(&mut heard, &view, me, listener, elapsed, &places, round);
    if cues.iter().any(|c| c.sound == Sound::Gong) {
        crate::dojo::strike(world, 1.);
    }
    if let Some(server) = world.get_resource::<AssetServer>().cloned() {
        for c in &cues {
            let handle = heard.handles.entry(c.take.file).or_insert_with(|| server.load(c.take.file)).clone();
            world.spawn((AudioPlayer::new(handle), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(c.volume)).with_speed(c.take.speed)));
        }
    }
    air(world, &mut heard, on);
    heard.played = cues;
    world.insert_resource(heard);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::audio::{AudioSource, Decodable, Source};
    use kungfu::{Event, FighterView, View};

    fn view(fighters: Vec<FighterView>, events: Vec<Event>) -> View {
        View { elapsed: 5., goal: 10, ends: 240., fighters, events }
    }
    fn fighter(id: u32, mv: Move, started: f32) -> FighterView {
        FighterView { id, mv: mv as u8, started, scale: 1., guard: 100., ..default() }
    }
    fn event(id: u32, kind: EventKind, mv: Move, amount: f32) -> Event {
        Event { id, kind: Some(kind), time: 5., by: 2, to: 1, mv: mv as u8, amount, p: [0., 1., 3.], ring: false, ..Default::default() }
    }
    fn kinds(cues: Vec<Cue>) -> Vec<Sound> {
        cues.into_iter().map(|c| c.sound).collect()
    }
    /// A round that is already under way (no start sting) and over nothing.
    const ON: Round = Round { over: false, won: false };
    fn started(heard: &mut Heard, places: &HashMap<u32, Vec3>, listener: Vec3) {
        // (Its clock is old: not a start.)
        let v = View { elapsed: 60., ..view(vec![], vec![]) };
        hear(heard, &v, 1, listener, 60., places, ON);
    }

    /// A wave file, decoded as the game decodes them (Bevy's own decoder, the wav feature): (channels, rate, samples).
    fn decoded(path: &str) -> (u16, u32, Vec<f32>) {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let d = AudioSource { bytes: bytes.into() }.decoder();
        (d.channels().get(), d.sample_rate().get(), d.collect())
    }
    fn assets(file: &str) -> String {
        format!("{}/assets/{file}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn every_mapped_file_exists_loads_in_the_games_decoder_and_none_is_left_unused() {
        let mut named = HashSet::new();
        for s in Sound::ALL {
            assert!(!s.takes().is_empty(), "{s:?}");
            assert!(s.volume() > 0.3 && s.volume() <= 0.65, "{s:?} sits under the guns");
            for t in s.takes() {
                named.insert(t.file);
                assert!(t.speed > 0.7 && t.speed < 1.4, "{t:?}");
                let (channels, rate, x) = decoded(&assets(t.file));
                // Mono 22050 Hz like the rest of the game's sounds, a sound and not a blip or a feature film, peaking well
                // under full scale but not whispering.
                assert_eq!((channels, rate), (1, 22050), "{}", t.file);
                let secs = x.len() as f32 / rate as f32;
                let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
                assert!(secs > 0.05 && secs < 5., "{}: {secs} s", t.file);
                assert!(peak > 0.25 && peak <= 0.9, "{}: peak {peak}", t.file);
                // No click at either end (the loop is meant to join up: its seam is a step like any other).
                let step = |a: f32, b: f32| (a - b).abs();
                if s == Sound::Ambience {
                    assert!(secs > 3., "{}: {secs} s of air to loop", t.file);
                    let typical = x.windows(2).map(|w| step(w[1], w[0])).sum::<f32>() / x.len() as f32;
                    assert!(step(x[0], x[x.len() - 1]) < 4. * typical + 0.01, "{}: the loop's seam", t.file);
                } else {
                    assert!(x[0].abs() < 0.02 && x[x.len() - 1].abs() < 0.02, "{}: ends {} and {}", t.file, x[0], x[x.len() - 1]);
                }
            }
        }
        // The recordings' folder holds only what is played (a take cut and forgotten is dead weight to download).
        let dir = assets("audio/kungfu");
        let mut files: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| format!("audio/kungfu/{}", e.unwrap().file_name().to_string_lossy())).collect();
        files.sort();
        let spare: Vec<&String> = files.iter().filter(|f| !named.contains(f.as_str())).collect();
        assert!(spare.is_empty(), "files nothing plays: {spare:?}");
        // The pools that are meant to vary have enough to.
        for (s, n) in [(Sound::Hit, 4), (Sound::HitHeavy, 3), (Sound::Block, 3), (Sound::Kiai, 3), (Sound::Scream, 3)] {
            let files: HashSet<&str> = s.takes().iter().map(|t| t.file).collect();
            assert!(s.takes().len() >= n && files.len() >= 2, "{s:?}");
        }
    }

    /// The compiler makes sure a new kind of event is counted here: this match has no catch-all.
    fn index(k: EventKind) -> usize {
        match k {
            EventKind::Hit => 0,
            EventKind::Blocked => 1,
            EventKind::Parried => 2,
            EventKind::Guardbroken => 3,
            EventKind::Grabbed => 4,
            EventKind::Knockdown => 5,
            EventKind::Ko => 6,
            EventKind::Dodged => 7,
            EventKind::Absorbed => 8,
            EventKind::Special => 9,
        }
    }
    #[test]
    fn every_kind_of_event_has_its_sounds_and_only_a_dodge_is_left_silent() {
        let every = [
            EventKind::Hit,
            EventKind::Blocked,
            EventKind::Parried,
            EventKind::Guardbroken,
            EventKind::Grabbed,
            EventKind::Knockdown,
            EventKind::Ko,
            EventKind::Dodged,
            EventKind::Absorbed,
            EventKind::Special,
        ];
        assert_eq!(every.map(index), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        for k in every {
            let got = sounds(&event(1, k, Move::TigerLight1, 4.));
            assert_eq!(got.is_empty(), k == EventKind::Dodged, "{k:?}: {got:?}");
        }
        assert!(sounds(&Event::default()).is_empty(), "an event of no kind");
        // The table in full: a hit by its size, a throw landing, a guard breaking with its rattle, a knockdown with its
        // grunt, a KO with its gong and its scream, a special with its shout.
        let of = |k, mv, amount| sounds(&event(1, k, mv, amount));
        assert_eq!(of(EventKind::Hit, Move::TigerLight1, 4.), vec![Sound::Hit]);
        assert_eq!(of(EventKind::Hit, Move::TigerHeavy1, 17.), vec![Sound::HitHeavy]);
        assert_eq!(of(EventKind::Hit, Move::Grab, 12.), vec![Sound::Throw, Sound::HitHeavy]);
        assert_eq!(of(EventKind::Blocked, Move::TigerLight1, 2.), vec![Sound::Block]);
        assert_eq!(of(EventKind::Parried, Move::TigerLight1, 0.), vec![Sound::Parry]);
        assert_eq!(of(EventKind::Guardbroken, Move::TigerHeavy2, 5.), vec![Sound::GuardBreak, Sound::Rattle]);
        assert_eq!(of(EventKind::Grabbed, Move::Grab, 0.), vec![Sound::Grab]);
        assert_eq!(of(EventKind::Knockdown, Move::TigerHeavy2, 0.), vec![Sound::Throw, Sound::Grunt]);
        assert_eq!(of(EventKind::Ko, Move::TigerHeavy2, 0.), vec![Sound::Gong, Sound::Scream]);
        assert_eq!(of(EventKind::Absorbed, Move::CraneLight1, 0.5), vec![Sound::Clang]);
        assert_eq!(of(EventKind::Special, Move::TigerSpecial, 0.), vec![Sound::Special]);
        // Every sound some event, move, or the round makes is a sound in the table (nothing is orphaned), and every
        // sound is made by something.
        let mut made: HashSet<Sound> = every.iter().flat_map(|k| sounds(&event(1, *k, Move::TigerLight1, 20.))).collect();
        made.extend(sounds(&event(1, EventKind::Hit, Move::Grab, 1.)));
        made.extend(sounds(&event(1, EventKind::Hit, Move::TigerLight1, 1.)));
        made.extend([Sound::Chi, Sound::Style, Sound::Start, Sound::Win, Sound::End, Sound::Ambience]);
        made.extend([Move::TigerLight1, Move::TigerHeavy1, Move::DodgeLeft].iter().flat_map(|m| swing(*m).into_iter().chain(shout(*m))));
        for s in Sound::ALL {
            assert!(made.contains(&s), "{s:?} is never played");
        }
    }

    #[test]
    fn a_strike_starting_whooshes_once_and_each_event_sounds_once() {
        let mut heard = Heard::default();
        let places = HashMap::from([(1, Vec3::new(0., 1., 3.)), (2, Vec3::new(0.2, 1., 3.))]);
        let listener = Vec3::new(0., 1., 3.);
        started(&mut heard, &places, listener);
        // The first frame only learns where everybody is.
        let idle = view(vec![fighter(1, Move::Idle, 0.), fighter(2, Move::Idle, 0.)], vec![]);
        assert!(hear(&mut heard, &idle, 1, listener, 5., &places, ON).is_empty());
        // Fighter 2 throws a tiger's heavy palm: a heavy whoosh and a kiai; fighter 1 a light jab: a light whoosh; fighter 1
        // then side steps, which whooshes as well.
        let swinging = view(vec![fighter(1, Move::TigerLight1, 4.98), fighter(2, Move::TigerHeavy1, 4.99)], vec![]);
        let got = kinds(hear(&mut heard, &swinging, 1, listener, 5., &places, ON));
        assert_eq!(got, vec![Sound::Whoosh, Sound::WhooshHeavy, Sound::Kiai]);
        assert!(hear(&mut heard, &swinging, 1, listener, 5.05, &places, ON).is_empty(), "once");
        let dodging = view(vec![fighter(1, Move::DodgeLeft, 5.06), fighter(2, Move::TigerHeavy1, 4.99)], vec![]);
        assert_eq!(kinds(hear(&mut heard, &dodging, 1, listener, 5.07, &places, ON)), vec![Sound::Whoosh]);
        // It lands, heavy: a heavy hit; then it is blocked, parried, a guard breaks, a Toad's iron body rings, a KO.
        let events = vec![
            event(1, EventKind::Hit, Move::TigerHeavy1, 17.),
            event(2, EventKind::Hit, Move::SnakeLight1, 4.),
            event(3, EventKind::Blocked, Move::TigerLight1, 2.),
            event(4, EventKind::Parried, Move::TigerLight1, 0.),
            event(5, EventKind::Guardbroken, Move::TigerHeavy2, 5.),
            event(6, EventKind::Absorbed, Move::CraneLight1, 0.5),
            event(7, EventKind::Grabbed, Move::Grab, 0.),
            event(8, EventKind::Hit, Move::Grab, 12.),
            event(9, EventKind::Ko, Move::TigerHeavy2, 0.),
            event(10, EventKind::Dodged, Move::TigerLight1, 0.),
        ];
        let got = kinds(hear(&mut heard, &view(dodging.fighters.clone(), events.clone()), 1, listener, 5.1, &places, ON));
        assert_eq!(
            got,
            vec![
                Sound::HitHeavy,
                Sound::Hit,
                Sound::Block,
                Sound::Parry,
                Sound::GuardBreak,
                Sound::Rattle,
                Sound::Clang,
                Sound::Grab,
                Sound::Throw,
                Sound::HitHeavy,
                Sound::Gong,
                Sound::Scream
            ]
        );
        assert!(hear(&mut heard, &view(dodging.fighters.clone(), events), 1, listener, 5.2, &places, ON).is_empty(), "each event once");
        // A new round numbers its events from 1 again.
        let got = hear(&mut heard, &view(dodging.fighters.clone(), vec![event(1, EventKind::Blocked, Move::TigerLight1, 2.)]), 1, listener, 1., &places, ON);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn a_throw_and_a_knockdown_and_a_special_sound_as_they_should() {
        let mut heard = Heard::default();
        let places = HashMap::new();
        started(&mut heard, &places, Vec3::new(0., 1., 3.));
        let events = vec![event(1, EventKind::Knockdown, Move::TigerHeavy2, 0.), Event { by: 3, ..event(2, EventKind::Special, Move::SnakeSpecial, 0.) }];
        let got = kinds(hear(&mut heard, &view(vec![], events), 1, Vec3::new(0., 1., 3.), 61., &places, ON));
        assert_eq!(got, vec![Sound::Throw, Sound::Grunt, Sound::Special]);
        // A fighter knocked out in the same moment as it falls screams and does not grunt as well.
        let ko = vec![event(3, EventKind::Knockdown, Move::TigerHeavy2, 0.), event(4, EventKind::Ko, Move::TigerHeavy2, 0.)];
        let got = kinds(hear(&mut Heard::default(), &view(vec![], ko), 1, Vec3::new(0., 1., 3.), 62., &places, ON));
        assert_eq!(got, vec![Sound::Throw, Sound::Gong, Sound::Scream]);
    }

    #[test]
    fn this_players_chi_filling_chimes_and_a_new_style_stings_and_far_off_is_quieter() {
        let mut heard = Heard::default();
        let places = HashMap::new();
        started(&mut heard, &places, Vec3::ZERO);
        let me = |chi: f32, style: u8| FighterView { id: 1, style, chi, scale: 1., ..default() };
        hear(&mut heard, &view(vec![me(90., 0)], vec![]), 1, Vec3::ZERO, 5., &places, ON);
        assert_eq!(kinds(hear(&mut heard, &view(vec![me(100., 0)], vec![]), 1, Vec3::ZERO, 5., &places, ON)), vec![Sound::Chi]);
        assert!(hear(&mut heard, &view(vec![me(100., 0)], vec![]), 1, Vec3::ZERO, 5., &places, ON).is_empty(), "once, while it stays full");
        assert_eq!(kinds(hear(&mut heard, &view(vec![me(40., 6)], vec![]), 1, Vec3::ZERO, 5., &places, ON)), vec![Sound::Style]);
        // A blow across the counter is a fifth as loud as one at hand; the gong is heard whole anywhere.
        assert_eq!((near(0.2), near(10.)), (1., 0.2));
        assert!(near(1.5) < 1. && near(1.5) > near(3.));
        let far = Event { p: [4., 1., 3.], ..event(1, EventKind::Hit, Move::TigerLight1, 7.) };
        let gong = Event { p: [4., 1., 3.], ..event(2, EventKind::Ko, Move::TigerLight1, 0.) };
        let got = hear(&mut Heard::default(), &view(vec![], vec![far, gong]), 1, Vec3::new(0., 1., 3.), 5., &places, ON);
        assert!((got[0].volume - Sound::Hit.volume() * near(4.)).abs() < 1e-6);
        assert_eq!((got[1].sound, got[1].volume), (Sound::Gong, Sound::Gong.volume()));
        assert_eq!((got[2].sound, got[2].volume), (Sound::Scream, Sound::Scream.volume() * near(4.)), "the scream fades with the distance, the gong does not");
    }

    #[test]
    fn the_round_has_a_sting_at_each_end_and_none_on_joining_late() {
        let mut heard = Heard::default();
        let places = HashMap::new();
        let at = |t: f32| View { elapsed: t, ..view(vec![], vec![]) };
        // The first look at a young round: its start; the same round going on: nothing more.
        assert_eq!(kinds(hear(&mut heard, &at(0.4), 1, Vec3::ZERO, 0.4, &places, ON)), vec![Sound::Start]);
        assert!(hear(&mut heard, &at(30.), 1, Vec3::ZERO, 30., &places, ON).is_empty());
        // It ends: a victory sting for this player's win, once, and the other sting for anyone else's (or a draw's).
        let won = Round { over: true, won: true };
        assert_eq!(kinds(hear(&mut heard, &at(31.), 1, Vec3::ZERO, 31., &places, won)), vec![Sound::Win]);
        assert!(hear(&mut heard, &at(32.), 1, Vec3::ZERO, 32., &places, won).is_empty(), "once");
        // The next round: its clock has run back: a start again, and its end is heard again, as the end.
        assert_eq!(kinds(hear(&mut heard, &at(0.2), 1, Vec3::ZERO, 0.2, &places, ON)), vec![Sound::Start]);
        assert_eq!(kinds(hear(&mut heard, &at(20.), 1, Vec3::ZERO, 20., &places, Round { over: true, won: false })), vec![Sound::End]);
        // Joining a round that is 80 s old is no start, but it still ends.
        let mut late = Heard::default();
        assert!(hear(&mut late, &at(80.), 1, Vec3::ZERO, 80., &places, ON).is_empty());
        assert_eq!(kinds(hear(&mut late, &at(81.), 1, Vec3::ZERO, 81., &places, won)), vec![Sound::Win]);
    }

    /// Nine fighters in a ring and one at the counter's far end all begin heavy strikes together: not a wall of kiais.
    #[test]
    fn a_crowd_of_fighters_is_not_a_wall_of_kiais() {
        let listener = Vec3::new(0., 1., 3.);
        let places: HashMap<u32, Vec3> = (1..=9).map(|id| (id, listener + Vec3::new(id as f32 * 0.1, 0., 0.))).chain([(10, listener + Vec3::new(6., 0., 0.))]).collect();
        let mut heard = Heard::default();
        started(&mut heard, &places, listener);
        let kiais = |heard: &mut Heard, mv: Move, started: f32, elapsed: f32| {
            let fighters = (1..=10).map(|id| fighter(id, mv, started)).collect();
            hear(heard, &view(fighters, vec![]), 1, listener, elapsed, &places, ON).iter().filter(|c| c.sound == Sound::Kiai).count()
        };
        kiais(&mut heard, Move::Idle, 0., 5.);
        // Ten begin at once: this player's own and two more (the crowd's limit), and none of those the 6 m off.
        assert_eq!(kiais(&mut heard, Move::TigerHeavy1, 5.0, 5.01), VOICE_CROWD.0);
        // 0.6 s on they strike again: the three are resting, and the rest are held by the crowd's second.
        assert_eq!(kiais(&mut heard, Move::TigerHeavy2, 5.6, 5.61), 0);
        // 1.1 s after the first three the crowd's second is over, so three others begin; the first three still rest.
        assert_eq!(kiais(&mut heard, Move::TigerHeavy1, 6.1, 6.11), VOICE_CROWD.0);
        // 0.2 s later the first three are rested but the crowd is full again: only this player's own is heard.
        assert_eq!(kiais(&mut heard, Move::TigerHeavy2, 6.3, 6.31), 1);
    }

    #[test]
    fn one_fighter_shouts_at_most_once_a_second_and_a_bit() {
        let listener = Vec3::new(0., 1., 3.);
        let places = HashMap::from([(1, listener), (2, listener + Vec3::new(0.3, 0., 0.))]);
        let mut heard = Heard::default();
        started(&mut heard, &places, listener);
        let heavy = |id: u32, mv: Move, started: f32| view(vec![fighter(1, Move::Idle, 0.), fighter(id, mv, started)], vec![]);
        let shouted = |heard: &mut Heard, v: &View, t: f32| hear(heard, v, 1, listener, t, &places, ON).iter().filter(|c| c.sound == Sound::Kiai).count();
        shouted(&mut heard, &heavy(2, Move::Idle, 0.), 6.);
        // Fighter 2 strikes heavy, again 0.5 s later, again at 1.1 s: only the first. At 1.3 s: it shouts again.
        assert_eq!(shouted(&mut heard, &heavy(2, Move::TigerHeavy1, 6.0), 6.0), 1);
        assert_eq!(shouted(&mut heard, &heavy(2, Move::TigerHeavy2, 6.5), 6.5), 0);
        assert_eq!(shouted(&mut heard, &heavy(2, Move::TigerHeavy1, 7.1), 7.1), 0, "1.1 s after its last");
        assert_eq!(shouted(&mut heard, &heavy(2, Move::TigerHeavy2, 7.3), 7.3), 1, "1.3 s after its last");
        // A light strike has no shout, a special's is its event's, and a far fighter is not heard at all.
        assert_eq!(shouted(&mut heard, &heavy(2, Move::TigerLight1, 9.), 9.), 0);
        let mut far = Heard::default();
        let places = HashMap::from([(1, listener), (2, listener + Vec3::new(VOICE_RANGE + 0.5, 0., 0.))]);
        started(&mut far, &places, listener);
        hear(&mut far, &heavy(2, Move::Idle, 0.), 1, listener, 6., &places, ON);
        assert_eq!(hear(&mut far, &heavy(2, Move::TigerHeavy1, 6.), 1, listener, 6., &places, ON).iter().filter(|c| c.sound == Sound::Kiai).count(), 0, "out of range");
        // This player's own is heard whatever the crowd.
        let (mut crowd, places) = (Heard::default(), HashMap::from([(1, listener)]));
        started(&mut crowd, &places, listener);
        for t in [6.0, 6.1, 6.2] {
            assert!(crowd.may_shout(40 + (t * 10.) as u32, t, 1., false, false));
        }
        assert!(!crowd.may_shout(50, 6.3, 1., false, false), "a fourth within a second");
        assert!(crowd.may_shout(1, 6.3, 0., true, false), "this player's own");
        assert!(crowd.may_shout(60, 6.4, 1., false, true), "a knockout's scream is never dropped");
        // A fighter's scream still counts against its next shout; and a new round's clock lets everyone shout again.
        assert!(!crowd.may_shout(60, 6.5, 0., true, false));
        assert!(crowd.may_shout(60, 0.5, 0., false, false), "a clock that has run back: a new round");
    }

    #[test]
    fn takes_vary_without_repeating_and_stay_within_their_pitch() {
        let mut heard = Heard::default();
        for s in [Sound::Hit, Sound::HitHeavy, Sound::Block, Sound::Kiai, Sound::Scream] {
            let mut seen = HashSet::new();
            let mut last = None;
            for i in 0..200u32 {
                let c = heard.cue(s, 1., i * 31);
                let index = s.takes().iter().position(|t| t.file == c.take.file && (t.speed - c.take.speed).abs() <= t.speed * PITCH + 1e-4).expect("one of its takes, within the pitch");
                assert_ne!(Some(index), last, "{s:?} repeated a take");
                last = Some(index);
                seen.insert(index);
                assert_eq!(c.volume, s.volume());
            }
            assert_eq!(seen.len(), s.takes().len(), "{s:?}: every take comes up in 200");
        }
        // A sound of one take, or one that never varies, is played as it is.
        let c = heard.cue(Sound::Gong, 1., 9);
        assert_eq!((c.take, c.volume), (Sound::Gong.takes()[0], Sound::Gong.volume()));
        let c = heard.cue(Sound::Start, 0.5, 9);
        assert_eq!((c.take.speed, c.volume), (1., 0.5 * Sound::Start.volume()));
    }

    /// The dojo's air: one loop for as long as a Kung Fu map is played (not a siege), stopped when it is not.
    #[test]
    fn the_dojos_air_loops_on_a_kung_fu_map_and_stops_with_it() {
        use bevy::audio::{PlaybackMode, PlaybackSettings};
        let mut app = crate::arena::test_app("sandbox botmode=idle");
        app.init_resource::<Heard>().add_systems(Update, (crate::arena::tick, listen).chain());
        let loops = |app: &mut App| app.world_mut().query::<&PlaybackSettings>().iter(app.world()).filter(|p| matches!(p.mode, PlaybackMode::Loop)).count();
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        assert_eq!(loops(&mut app), 0, "the siege's kitchen has no bamboo");
        let dojo = kitchen_sim::map::Map::parse(include_str!("../assets/maps/kung-fu-dojo.json")).unwrap();
        crate::arena::load_map(app.world_mut(), dojo);
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        assert!(app.world().resource::<Game>().kung_fu());
        assert_eq!(loops(&mut app), 1, "one loop in the dojo");
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        assert_eq!(loops(&mut app), 1, "and still one");
        crate::arena::load_map(app.world_mut(), kitchen_sim::map::Map::bundled());
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        assert_eq!(loops(&mut app), 0, "stopped with the Kung Fu round");
    }
}

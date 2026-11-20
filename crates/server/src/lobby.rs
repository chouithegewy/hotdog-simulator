// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The multiplayer lobby: who is here, what they are called, the colours they wear, who is ready, the text
//! chat and the signalling for the voice chat, the leader and the next game. No sockets and no timers of its own:
//! the game loop (hub.rs) hands it messages and a way to answer, and calls `tick` beside the game's. The game owns
//! the stage (Lobby, Opening, the round), the countdown and who is ready; this owns everything a player is apart
//! from their hotdog.
//!
//! A member is one person. A browser has two sockets: its lobby socket (the page's lobby.js: `enter`, then
//! `handle`) and its game socket (the WebAssembly game: `claim` with the token the lobby socket was given, then
//! `seat`). A client with no page (the native one) has only the game socket and is a `guest`: a member with a name
//! and a colour but no chat and no voice.
//!
//! From a page:   {type:'hello',name,color,team?,token?}  {type:'profile',name?,color?}  {type:'ready',ready}
//!                {type:'chat',text}  {type:'signal',to,data}  {type:'speaking',level}  {type:'mic',state}
//!                {type:'team',team}  {type:'choose',mode?,map?}
//! To a page:     {type:'welcome',id,token,colours,limits,history,mode,teams}
//!                {type:'roster',stage,countdown,capacity,members,mode,leader,choice,playing,maps,...}
//!                {type:'chat',from,name,color,text}  {type:'system',text}  {type:'notice',text}
//!                {type:'signal',from,data}  {type:'voice',id,level}  {type:'error',message}
//!
//! The leader and the choice. One server plays every map: the first member seated (with a page) leads the lobby
//! until they leave, then the next in the order they arrived; the leader chooses the mode (siege, Forts, Free For
//! All, Fully Loaded or Kung Fu) and the map of the next round from the maps in assets/maps (`scan`), the two apart, in
//! the lobby only, and a change makes everyone unready. The mode is the game's, not the map's (kitchen_sim::rules
//! `Mode`): a choice is an entry of a map with the `mode` it is played in, and a map's own `mode` is only the mode it
//! is played in when nobody has chosen one. The gun modes play wherever the map can hold them (`refusal`: Free For All
//! and the siege on every map but the dojo, Forts on a map with the Forts layout); Fully Loaded (kitchen_sim::cards)
//! has one table, on Counter Siege's microwave, and Kung Fu (kitchen_sim::kungfu) one ring, the dojo's, which is played
//! as nothing else. A mode chosen keeps the map chosen where it can be played there, and goes to the map last chosen
//! for it (else its default, else its first) where not. A map chosen alone is played in the mode the leader chose
//! (`mode_set`) when that is a gun mode, and refused, with the reason, when it cannot be played so; before the leader
//! has chosen a mode (or while Fully Loaded or Kung Fu is chosen) it is played in its own. Random draws from the maps
//! that can hold the mode. When the fridge opens on a map (or a mode) other than the one being played, a fresh game is
//! built for it and takes over with everyone seated as they were (`switch`); the game loop tells every client the new
//! map.
//!
//! The dev sandbox (a server started with --dev): the first game socket to join an empty game may ask for it
//! (`sandbox`), and the game is built afresh by its rules on the map it names (or the one being played), with no
//! lobby; when the last player has gone, the lobby's own game comes back (`unsandbox`).
use crate::game::{COLOURS, Game, Phase, Profile, Settings};
use crate::outbox::Outbox;
use crate::protocol::{self, is};
use crate::text::{CHAT, NAME, clean_chat, clean_name, unique_name};
use axum::extract::ws::Utf8Bytes;
use kitchen_sim::map::{FORT_PLAYERS, MODES, Map, TEAM_NAMES, TEAMS};
use kitchen_sim::net::{Choice, Frame};
use kitchen_sim::rules::{Mode, Rules};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The places in the lobby (in either mode: a Forts round has six hotdogs, so at least one is a bot).
pub const CAPACITY: usize = 5;
/// A member may send RATE.0 chat lines every RATE.1 milliseconds. A signalling message (a WebRTC offer, answer or
/// candidate) is at most SIGNAL characters of JSON. HISTORY chat lines are kept for whoever arrives next. A voice
/// level that is not renewed within QUIET milliseconds is silence. A member who gives no name is FALLBACK.
pub const RATE: (usize, f64) = (5, 5000.);
pub const SIGNAL: usize = 12000;
pub const HISTORY: usize = 30;
pub const QUIET: f64 = 700.;
pub const FALLBACK: &str = "HOTDOG";
/// RANDOM is the choice of a map of the mode drawn at random each time the fridge opens (never the one played last,
/// where the mode has another): it is listed first among each mode's maps, shows as "Random" until the fridge
/// opens, and the name is kept for it (a file of that name is not listed).
pub const RANDOM: &str = "random";
/// The map a mode starts on when the leader picks the mode and has not picked one of its maps yet (Fully Loaded's
/// only map: the card table stands on Counter Siege's microwave).
pub fn default_map(mode: &str) -> &'static str {
    match mode {
        "forts" => "pillow-fort",
        KUNGFU => "kung-fu-dojo",
        _ => "counter-siege",
    }
}
pub fn mode_name(mode: &str) -> &'static str {
    match mode {
        "siege" => "Siege",
        "forts" => "Forts",
        FFA => "Free For All",
        CARDS => "Fully Loaded",
        KUNGFU => "Kung Fu",
        _ => "such",
    }
}
/// A game's map as the server's console prints it: "Counter Siege (siege)", with the mode the game is played in (the
/// rules', or the map's own), and Fully Loaded's display name for a game played as that.
pub fn played(game: &Game) -> String {
    let mode = if game.rules().cards { mode_name(CARDS) } else { game.feast.mode().name() };
    format!("{} ({mode})", game.map.name)
}
/// The modes the leader chooses from (kitchen_sim::rules `Mode`): the siege, Forts and Free For All on the maps that can
/// hold them, Fully Loaded on Counter Siege, and Kung Fu on the dojo.
pub const FFA: &str = "ffa";
pub const CARDS: &str = "cards";
pub const KUNGFU: &str = kitchen_sim::kungfu::MODE;
pub const PLAYS: [&str; 5] = ["siege", "forts", FFA, CARDS, KUNGFU];
/// What a game of an entry is played by: the rules of the mode it is played in (kitchen_sim::rules `Rules::playing`:
/// naming no mode where it is the map's own siege or Forts, as a game always said).
pub fn rules_for(entry: &Entry) -> Rules {
    let Some(mode) = Mode::named(&entry.mode) else {
        return Rules::default();
    };
    match entry.map.as_deref() {
        Some(map) => Rules::playing(mode, map),
        None => Rules::of(Some(mode)),
    }
}
/// Why the map of `entry` cannot be played as `mode` in this lobby (None: it can): Fully Loaded is played on Counter
/// Siege alone, and the rest where the map says (kitchen_sim `Map::refuses`: the dojo is Kung Fu's alone, the siege
/// and Free For All play every other map, Forts a map with the Forts layout). Said as a sentence.
pub fn refusal(entry: &Entry, mode: &str) -> Option<String> {
    let Some(played) = Mode::named(mode) else {
        return Some("There is no such mode.".into());
    };
    let map = entry.map.as_deref()?;
    if played == Mode::Cards && entry.name != default_map(CARDS) {
        return Some(format!("{} is played on Counter Siege.", mode_name(CARDS)));
    }
    map.refuses(played).map(|why| format!("{why}: it cannot be played as {}.", mode_name(mode)))
}
/// The modes the map of `entry` can be played in, by name.
pub fn modes(entry: &Entry) -> Vec<&'static str> {
    PLAYS.into_iter().filter(|mode| refusal(entry, mode).is_none()).collect()
}
/// Forts: a member asks for a team ('mustard' or 'ketchup') or leaves it to the server ('auto').
pub const WISHES: [&str; 3] = ["auto", "mustard", "ketchup"];
/// A map's name in the folder, and so in the leader's choice: a bare word, so that no path can be named.
pub fn map_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A map the leader may choose (or Random): the file's name without .json, the name written in it, its mode and
/// look, and the map itself with its canonical text.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub title: String,
    pub mode: String,
    pub theme: String,
    pub random: bool,
    pub map: Option<Arc<Map>>,
    pub text: Arc<str>,
}
impl Entry {
    pub fn listing(name: &str, map: Map) -> Entry {
        let text = map.to_text().into();
        Entry { name: name.into(), title: map.name.clone(), mode: map.mode.clone(), theme: map.theme.clone(), random: false, map: Some(Arc::new(map)), text }
    }
    pub fn randomly(mode: &str) -> Entry {
        Entry { name: RANDOM.into(), title: "Random".into(), mode: mode.into(), theme: String::new(), random: true, map: None, text: "".into() }
    }
    /// The entry played as `mode` (any mode its map can hold: `refusal`).
    pub fn played_as(mut self, mode: &str) -> Entry {
        self.mode = mode.into();
        self
    }
    /// The entry as pages are shown it: {name, title, mode, theme}, and `random` for Random.
    pub fn shown(&self) -> Value {
        serde_json::to_value(self.choice()).unwrap_or_default()
    }
    /// A map as the leader's list shows it: as `shown` (with the mode it was made for), the modes it can be played in
    /// (`modes`), and why it cannot be played in each of the others (`why`, by mode).
    pub fn listed(&self) -> Value {
        let mut out = self.shown();
        let why: serde_json::Map<String, Value> = PLAYS.iter().filter_map(|mode| refusal(self, mode).map(|why| (mode.to_string(), json!(why)))).collect();
        out["modes"] = json!(modes(self));
        out["why"] = Value::Object(why);
        out
    }
    /// The mode the entry's map was made for (its `mode` tag), which it is played in when nobody chose another.
    pub fn own(&self) -> String {
        self.map.as_ref().map_or_else(|| self.mode.clone(), |m| m.mode.clone())
    }
    /// The entry as frames name it.
    pub fn choice(&self) -> Choice {
        Choice { name: self.name.clone(), title: self.title.clone(), mode: self.mode.clone(), theme: self.theme.clone(), random: self.random }
    }
}
/// What the folder of maps holds: the maps that can be played, and why each that cannot be is not, by name.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    pub maps: Vec<Entry>,
    pub skipped: Vec<(String, String)>,
}
/// The order maps are listed in: by mode (siege first), then by title, then by name, as a person reads them.
fn collate(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| s.to_lowercase();
    key(a).cmp(&key(b)).then_with(|| {
        // Lowercase before capitals, as the Node server's localeCompare put them.
        let flip = |s: &str| s.chars().map(|c| if c.is_lowercase() { c.to_uppercase().next().unwrap_or(c) } else { c.to_lowercase().next().unwrap_or(c) }).collect::<String>();
        flip(a).cmp(&flip(b))
    })
}
/// Every file in `dir` whose name is a bare word and whose contents are a map that can be played.
pub fn scan(dir: &Path) -> Catalogue {
    let mut catalogue = Catalogue::default();
    let Ok(files) = std::fs::read_dir(dir) else {
        return catalogue;
    };
    for file in files.flatten() {
        let file = file.file_name().to_string_lossy().to_string();
        let name = file.strip_suffix(".json").unwrap_or("");
        if !map_name(name) {
            continue;
        }
        if name == RANDOM {
            catalogue.skipped.push((name.into(), "the name random is kept for a map drawn at random".into()));
            continue;
        }
        let read = std::fs::read_to_string(dir.join(&file)).map_err(|error| error.to_string());
        match read.and_then(|text| Map::parse(&text).map_err(|error| error.0)) {
            Ok(map) => catalogue.maps.push(Entry::listing(name, map)),
            Err(reason) => catalogue.skipped.push((name.into(), reason.encode_utf16().take(160).map(|u| char::from_u32(u as u32).unwrap_or('\u{fffd}')).collect())),
        }
    }
    let rank = |mode: &str| MODES.iter().position(|m| *m == mode).unwrap_or(MODES.len());
    catalogue.maps.sort_by(|a, b| rank(&a.mode).cmp(&rank(&b.mode)).then_with(|| collate(&a.title, &b.title)).then_with(|| collate(&a.name, &b.name)));
    catalogue
}

/// One person in the lobby.
pub struct Member {
    pub id: u32,
    pub token: String,
    /// The page's lobby socket, while it is open (a guest has none).
    pub send: Option<Outbox>,
    /// Whether the member came with a page (chat, voice and the choice of game are a page's).
    pub voice: bool,
    /// The hotdog its game socket plays, once seated.
    pub player: Option<u32>,
    pub name: String,
    pub color: String,
    /// Its microphone ('none', 'muted' or 'live'), how loudly it speaks and when it last said so (ms).
    pub mic: String,
    pub level: f64,
    pub spoke: f64,
    /// When it sent its recent chat lines (ms).
    pub lines: Vec<f64>,
    /// Forts: the team it asks for ('auto', 'mustard' or 'ketchup').
    pub team: String,
}

/// How a lobby finds its maps and keeps time; replaceable in tests.
pub struct Options {
    /// The maps there are to choose from (by default `scan` of assets/maps).
    pub maps: Box<dyn Fn() -> Catalogue + Send>,
    /// The name the map the game plays goes by when it is not one of the folder's (the server's --map file).
    pub choice: Option<String>,
    /// Start on Random, in the mode of the game's map (the server's --map random).
    pub random: bool,
    /// Whether the mode the game is played in was chosen (the server's --mode): a map chosen alone then keeps it.
    pub chosen: bool,
    /// Milliseconds, a new member's token, and the dice (0 to 1).
    pub now: Box<dyn Fn() -> f64 + Send>,
    pub token: Box<dyn FnMut() -> String + Send>,
    pub draw: Box<dyn FnMut() -> f64 + Send>,
}
impl Options {
    /// Maps from `folder`, the clock, random tokens and dice.
    pub fn new(folder: PathBuf) -> Options {
        let start = std::time::Instant::now();
        Options {
            maps: Box::new(move || scan(&folder)),
            choice: None,
            random: false,
            chosen: false,
            now: Box::new(move || start.elapsed().as_secs_f64() * 1000.),
            token: Box::new(|| {
                let bytes: [u8; 16] = rand::random();
                bytes.iter().map(|b| format!("{b:02x}")).collect()
            }),
            draw: Box::new(rand::random::<f64>),
        }
    }
}

/// What a page may relay to another: a session description or an ICE candidate, of plain strings and small numbers.
fn signal(data: &Value) -> Option<Value> {
    let data = data.as_object()?;
    let mut out = serde_json::Map::new();
    if let Some(description) = data.get("description").and_then(Value::as_object) {
        let kind = description.get("type").and_then(Value::as_str).filter(|kind| ["offer", "answer", "rollback"].contains(kind));
        let sdp = match description.get("sdp") {
            None | Some(Value::Null) => Some(""),
            Some(Value::String(sdp)) => Some(sdp.as_str()),
            Some(_) => None,
        };
        if let (Some(kind), Some(sdp)) = (kind, sdp) {
            out.insert("description".into(), json!({"type": kind, "sdp": sdp}));
        }
    }
    match data.get("candidate") {
        Some(Value::Null) => {
            out.insert("candidate".into(), Value::Null);
        }
        Some(Value::Object(c)) => {
            if let Some(candidate) = c.get("candidate").and_then(Value::as_str) {
                let text = |key: &str| c.get(key).and_then(Value::as_str).map_or(Value::Null, |s| json!(s));
                let line = c.get("sdpMLineIndex").and_then(protocol::integer).map_or(Value::Null, |n| json!(n as i64));
                out.insert("candidate".into(), json!({"candidate": candidate, "sdpMid": text("sdpMid"), "sdpMLineIndex": line, "usernameFragment": text("usernameFragment")}));
            }
        }
        _ => {}
    }
    if out.is_empty() {
        return None;
    }
    let out = Value::Object(out);
    // Measured as the page measures it: characters of JSON.
    (protocol::encode(&out).encode_utf16().count() <= SIGNAL).then_some(out)
}
/// Forts sides as the game makes them up when the match starts (game.rs `muster`): `seated` (in the order of
/// their hotdogs) take the side they asked for while it has room and the smaller one otherwise. By member id.
fn muster(seated: &[(u32, &str)], half: usize) -> BTreeMap<u32, u8> {
    let (mut count, mut side) = ([0usize; 2], BTreeMap::new());
    let seat = |id: u32, mut t: usize, count: &mut [usize; 2], side: &mut BTreeMap<u32, u8>| {
        if count[t] >= half {
            t = 1 - t;
        }
        side.insert(id, t as u8);
        count[t] += 1;
    };
    for &(id, wish) in seated {
        if let Some(t) = TEAMS.iter().position(|team| *team == wish) {
            seat(id, t, &mut count, &mut side);
        }
    }
    for &(id, _) in seated {
        if !side.contains_key(&id) {
            let t = if count[0] <= count[1] { 0 } else { 1 };
            seat(id, t, &mut count, &mut side);
        }
    }
    side
}

pub struct Lobby {
    pub game: Game,
    settings: Settings,
    pub members: BTreeMap<u32, Member>,
    next_member: u32,
    history: Vec<Value>,
    /// The roster last sent ('' to send it again at the next tick).
    sent: String,
    /// The game's stage at the last tick.
    stage: &'static str,
    scanner: Box<dyn Fn() -> Catalogue + Send>,
    pub maps: Vec<Entry>,
    pub skipped: Vec<(String, String)>,
    /// Maps that are not in the folder (the server's --map file), listed after the folder's.
    extra: Vec<Entry>,
    leader: Option<u32>,
    /// What is being played; what the leader has chosen for the next round (the same until the leader picks another,
    /// or Random), and whether its mode was chosen (by the leader, or the server's --mode: a map chosen alone keeps a gun
    /// mode that was); the map last chosen for each mode; and the map the last round played.
    pub current: Entry,
    pub choice: Entry,
    pub mode_set: bool,
    picked: HashMap<String, String>,
    played: Option<String>,
    /// The dev sandbox: what was being played and chosen before it took over, while it plays.
    before: Option<(Entry, Entry)>,
    now: Box<dyn Fn() -> f64 + Send>,
    token: Box<dyn FnMut() -> String + Send>,
    draw: Box<dyn FnMut() -> f64 + Send>,
}

impl Lobby {
    pub fn new(game: Game, settings: Settings, options: Options) -> Lobby {
        let Options { maps, choice, random, chosen, now, token, draw } = options;
        let stage = game.stage();
        let mut lobby = Lobby {
            game,
            settings,
            members: BTreeMap::new(),
            next_member: 1,
            history: vec![],
            sent: String::new(),
            stage,
            scanner: maps,
            maps: vec![],
            skipped: vec![],
            extra: vec![],
            leader: None,
            current: Entry::randomly("siege"),
            choice: Entry::randomly("siege"),
            mode_set: chosen,
            picked: HashMap::new(),
            played: None,
            before: None,
            now,
            token,
            draw,
        };
        lobby.catalogue();
        // A map that is not in the folder is listed after the folder's, under its file's name (or its title's), so
        // that it can be chosen again.
        let text = lobby.game.text.clone();
        lobby.current = match lobby.maps.iter().find(|e| *e.text == *text) {
            Some(found) => found.clone(),
            None => {
                let wanted = choice.unwrap_or_else(|| lobby.game.map.name.clone()).to_lowercase();
                let mut base = String::new();
                for c in wanted.chars() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        base.push(c);
                    } else if !base.ends_with('\u{0}') {
                        base.push('\u{0}');
                    }
                }
                let base = base.replace('\u{0}', "-");
                let base = base.trim_matches('-');
                let base = if base.is_empty() { "map".to_string() } else { base.to_string() };
                let (mut name, mut n) = (base.clone(), 2);
                while lobby.find(&name).is_some() || name == RANDOM {
                    name = format!("{base}-{n}");
                    n += 1;
                }
                let entry = Entry::listing(&name, lobby.game.map.clone());
                lobby.extra.push(entry.clone());
                lobby.catalogue();
                entry
            }
        };
        // A game the lobby is given in a mode other than its map's own (the server's --mode) is listed as played so;
        // Fully Loaded has no Random.
        let playing = if lobby.game.rules().cards { CARDS } else { lobby.game.feast.mode().name() };
        lobby.current = lobby.current.clone().played_as(playing);
        let cards = playing == CARDS;
        lobby.choice = if random && !cards { Entry::randomly(&lobby.current.mode) } else { lobby.current.clone() };
        lobby.picked.insert(lobby.choice.mode.clone(), lobby.choice.name.clone());
        lobby
    }
    fn list(&self) -> impl Iterator<Item = &Member> {
        self.members.values()
    }
    pub fn member(&self, id: u32) -> Option<&Member> {
        self.members.get(&id)
    }
    /// The hotdog a member plays, if it is seated in the game being played.
    pub fn player(&self, member: u32) -> Option<u32> {
        self.members.get(&member)?.player.filter(|p| self.game.player(*p).is_some())
    }
    fn full(&self) -> bool {
        self.members.len() >= CAPACITY
    }
    /// The names nobody else may take: the other members' and the bots'.
    fn taken(&self, except: Option<u32>) -> Vec<String> {
        let mut names: Vec<String> = self.list().filter(|m| Some(m.id) != except).map(|m| m.name.clone()).collect();
        names.extend(kitchen_sim::feast::NAMES.iter().map(|n| n.to_string()));
        names.extend(kitchen_sim::forts::NAMES.iter().flatten().map(|n| n.to_string()));
        names
    }
    /// Whether the lobby has teams: the next round (the one the leader has chosen) is Forts.
    pub fn forts(&self) -> bool {
        self.choice.mode == "forts"
    }
    /// The next round as the server's console prints it when it starts: the map the leader has chosen, or Random and
    /// the mode it draws a map of; a choice of Fully Loaded, which is a map (Counter Siege) played as that, says so.
    pub fn next_up(&self) -> String {
        if self.choice.random {
            format!("Random ({})", self.choice.mode)
        } else if self.choice.mode == CARDS {
            format!("{} ({})", self.choice.name, mode_name(CARDS))
        } else {
            self.choice.name.clone()
        }
    }
    /// The mode of the next round as pages name it: siege, forts, ffa, cards or kungfu.
    pub fn mode(&self) -> &'static str {
        PLAYS.iter().find(|m| **m == self.choice.mode).copied().unwrap_or("siege")
    }
    /// Whether a join may ask for the dev sandbox (the server's --dev).
    pub fn dev(&self) -> bool {
        self.settings.dev
    }

    // ----- The dev sandbox.

    /// A join asks for the sandbox (`asked`: the page's address as keys and values; kitchen_sim::rules, with `mode`
    /// siege, forts, ffa or cards and `map` a map's name): the game is built afresh by its rules on the map it names, or the
    /// default map of the mode it names, or else the map being played, with no lobby. Only on a --dev server and only
    /// while nobody plays (the hub asks); returns whether the sandbox took over.
    pub fn sandbox(&mut self, asked: &Value) -> bool {
        let Some(fields) = asked.as_object() else {
            return false;
        };
        let text = |v: &Value| match v {
            Value::String(s) => s.clone(),
            Value::Bool(true) | Value::Null => String::new(),
            Value::Bool(false) => "off".into(),
            other => other.to_string(),
        };
        let pairs: Vec<(String, String)> = fields.iter().map(|(k, v)| (k.clone(), text(v))).collect();
        let mut rules = Rules::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        rules.sandbox = true;
        let wanted = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.to_ascii_lowercase());
        self.catalogue();
        let mode = wanted("mode").filter(|m| PLAYS.contains(&m.as_str()));
        let base = self.before.as_ref().map_or(&self.current, |(current, _)| current).clone();
        let entry = match (wanted("map").and_then(|name| self.find(&name).cloned()), mode) {
            (Some(entry), _) => entry,
            // Fully Loaded has its table on Counter Siege.
            (None, _) if rules.cards => self.fully_loaded().unwrap_or(base.clone()),
            // A mode the map being played cannot hold: the mode's map (as the leader's choice of it would be).
            (None, Some(mode)) if base.random || refusal(&base, &mode).is_some() => self.map_for(&mode).unwrap_or(base.clone()),
            _ if base.random => self.maps.first().cloned().unwrap_or(base.clone()),
            _ => base.clone(),
        };
        let map = entry.map.as_deref().cloned().unwrap_or_else(|| self.game.map.clone());
        // (The siege or Forts on a map made for it says nothing of it, as a game always did: `Rules::playing`.)
        if rules.mode().is_some_and(|m| matches!(m, Mode::Siege | Mode::Forts) && m == Mode::made_for(&map)) {
            (rules.siege, rules.forts) = (false, false);
        }
        let mut game = Game::with_rules(map, &self.settings, rules);
        // (Listed as the mode the game plays: the rules', where the map can hold it, or else the map's own.)
        let entry = entry.played_as(if game.rules().cards { CARDS } else { game.feast.mode().name() });
        game.feast.round = self.game.feast.round + 1;
        self.game = game;
        if self.before.is_none() {
            self.before = Some((self.current.clone(), self.choice.clone()));
        }
        (self.current, self.choice) = (entry.clone(), entry);
        self.sent.clear();
        true
    }
    /// Nobody plays the sandbox any more: the lobby's own game comes back, on what was being played before it.
    pub fn unsandbox(&mut self) {
        let Some((current, choice)) = self.before.take() else {
            return;
        };
        let map = current.map.as_deref().cloned().unwrap_or_else(|| self.game.map.clone());
        let mut game = Game::with_rules(map, &self.settings, rules_for(&current));
        game.feast.round = self.game.feast.round + 1;
        self.game = game;
        (self.current, self.choice) = (current, choice);
        self.sent.clear();
    }

    // ----- The leader and the choice.

    /// The maps there are to choose from, looked at again in the folder.
    pub fn catalogue(&mut self) -> &[Entry] {
        let Catalogue { maps, skipped } = (self.scanner)();
        self.skipped = skipped;
        let extra = &self.extra;
        self.maps = maps.into_iter().filter(|e| !extra.iter().any(|x| x.name == e.name)).chain(extra.iter().cloned()).collect();
        &self.maps
    }
    fn find(&self, name: &str) -> Option<&Entry> {
        self.maps.iter().find(|e| e.name == name)
    }
    /// The one place Fully Loaded is played: Counter Siege of the folder, as an entry played as Fully Loaded.
    fn fully_loaded(&self) -> Option<Entry> {
        self.find(default_map(CARDS)).cloned().map(|e| e.played_as(CARDS))
    }
    /// The map a mode goes to when the map chosen cannot hold it: the one last chosen for it, else its default, else the
    /// first that can; played as the mode. None when no map can.
    fn map_for(&self, mode: &str) -> Option<Entry> {
        [self.picked.get(mode).cloned(), Some(default_map(mode).to_string())]
            .into_iter()
            .flatten()
            .filter_map(|name| self.find(&name).cloned())
            .chain(self.maps.iter().cloned())
            .find(|e| refusal(e, mode).is_none())
            .map(|e| e.played_as(mode))
    }
    /// The maps Random draws from for `mode`: those that can hold it, played as it.
    fn pool(&self, mode: &str) -> Vec<Entry> {
        self.maps.iter().filter(|e| refusal(e, mode).is_none()).cloned().map(|e| e.played_as(mode)).collect()
    }
    /// A member who can lead: one with a page (the page is where the game is chosen), seated, and not sitting out a
    /// round.
    fn eligible(&self, member: &Member) -> bool {
        member.voice && member.player.is_some_and(|p| self.game.player(p).is_some() && !self.game.waiting(p))
    }
    /// The leader, brought up to date: the one there is while they can lead, and otherwise the first who can, in the
    /// order they arrived. None for nobody.
    pub fn lead(&mut self) -> Option<u32> {
        if let Some(current) = self.leader.and_then(|id| self.members.get(&id))
            && self.eligible(current) {
                return Some(current.id);
            }
        let next = self.list().find(|m| self.eligible(m)).map(|m| (m.id, m.name.clone()));
        if next.as_ref().map(|n| n.0) != self.leader {
            self.leader = next.as_ref().map(|n| n.0);
            self.sent.clear();
            if let Some((_, name)) = &next {
                self.system(&format!("{name} leads the lobby and chooses the game"));
            }
        }
        next.map(|n| n.0)
    }
    /// The leader chooses what the next round plays: a mode, a map, or both, the two apart (see the module's
    /// documentation). A mode keeps the map chosen where it can be played there, and otherwise goes to its own map
    /// (`map_for`). A map (by its file's name, or RANDOM: one of the mode's maps drawn each time the fridge opens) is
    /// played in the mode chosen with it, or else in the gun mode the leader chose before (`mode_set`), or else in its
    /// own; one that cannot be played in the mode is refused, with the reason. Fully Loaded has Counter Siege only: it,
    /// and Random, are that map, and another is refused when Fully Loaded is asked for with it (a map chosen alone while
    /// Fully Loaded or Kung Fu is chosen is played in its own mode). Only the leader, only in the lobby before the
    /// countdown. A change makes everyone unready. Returns whether the choice changed; whoever is refused is told why.
    pub fn choose(&mut self, member: u32, mode: Option<&Value>, map: Option<&Value>) -> bool {
        let leader = self.lead();
        let refuse = |lobby: &Lobby, text: &str| {
            lobby.tell(member, &json!({"type": "notice", "text": text}));
            false
        };
        if Some(member) != leader {
            let text = match leader.and_then(|id| self.members.get(&id)) {
                Some(leader) => format!("Only the leader, {}, can choose the game.", leader.name),
                None => "Only the leader can choose the game.".into(),
            };
            return refuse(self, &text);
        }
        if self.game.phase != Phase::Lobby || self.game.countdown.is_some() {
            return refuse(self, "The game can only be changed in the lobby, before the countdown.");
        }
        let mode = match mode {
            None => None,
            Some(Value::String(m)) if PLAYS.contains(&m.as_str()) => Some(m.clone()),
            Some(_) => return refuse(self, "There is no such mode."),
        };
        let map = match map {
            None => None,
            Some(Value::String(m)) if map_name(m) => Some(m.clone()),
            Some(_) => return refuse(self, "That is not the name of a map."),
        };
        let asked = mode.is_some();
        // A map chosen with no mode is played in the mode chosen now: a gun mode the leader chose (a map that cannot hold
        // it is refused below), or Fully Loaded with Counter Siege (or Random, which is it). Otherwise, before a mode has
        // been chosen, or with any other map while Fully Loaded or Kung Fu is, it is played in its own mode, as a link to
        // it means.
        let mode = mode.or_else(|| {
            let chosen = self.choice.mode.as_str();
            let keeps = match chosen {
                CARDS => map.as_deref().is_some_and(|name| name == RANDOM || name == default_map(CARDS)),
                KUNGFU => false,
                _ => self.mode_set || map.as_deref() == Some(RANDOM),
            };
            keeps.then(|| chosen.to_string())
        });
        // Fully Loaded is played on Counter Siege: that, or Random, or no map, is it; any other is not.
        let map = if mode.as_deref() == Some(CARDS) {
            if map.as_deref().is_some_and(|name| name != RANDOM && name != default_map(CARDS)) {
                return refuse(self, &format!("{} is played on Counter Siege.", mode_name(CARDS)));
            }
            Some(default_map(CARDS).to_string())
        } else {
            map
        };
        if mode.is_none() && map.is_none() {
            return refuse(self, "Choose a mode or a map.");
        }
        self.catalogue();
        let entry = match &map {
            Some(name) if name == RANDOM => {
                let mode = mode.clone().unwrap_or_else(|| self.choice.mode.clone());
                if self.pool(&mode).is_empty() {
                    return refuse(self, &format!("There is no {} map to play.", mode_name(&mode)));
                }
                Entry::randomly(&mode)
            }
            Some(name) => {
                let Some(entry) = self.find(name).cloned() else {
                    let text = match self.skipped.iter().find(|(skipped, _)| skipped == name) {
                        Some((_, reason)) => format!("The map {name} cannot be played: {reason}"),
                        None => format!("There is no map called {name}."),
                    };
                    return refuse(self, &text);
                };
                let mode = mode.clone().unwrap_or_else(|| entry.own());
                if let Some(why) = refusal(&entry, &mode) {
                    return refuse(self, &why);
                }
                entry.played_as(&mode)
            }
            None => {
                // A mode alone: on the map chosen, where it can be played there; else on the mode's own map.
                let mode = mode.clone().unwrap_or_default();
                let kept = if self.choice.random {
                    (!self.pool(&mode).is_empty() && mode != CARDS).then(|| Entry::randomly(&mode))
                } else {
                    self.find(&self.choice.name).cloned().filter(|e| refusal(e, &mode).is_none()).map(|e| e.played_as(&mode))
                };
                match kept.or_else(|| self.map_for(&mode)) {
                    Some(entry) => entry,
                    None => return refuse(self, &format!("There is no {} map to play.", mode_name(&mode))),
                }
            }
        };
        if asked {
            self.mode_set = true;
        }
        if entry.name == self.choice.name && entry.mode == self.choice.mode {
            return false;
        }
        self.picked.insert(entry.mode.clone(), entry.name.clone());
        self.choice = entry;
        let seated: Vec<u32> = self.list().filter_map(|m| m.player).collect();
        for p in seated {
            if self.game.player(p).is_some_and(|p| p.ready) {
                self.game.ready(p, false);
            }
        }
        let what = if self.choice.random { "a map drawn at random".to_string() } else { format!("{} ({})", self.choice.title, self.choice.theme) };
        let name = self.members.get(&member).map(|m| m.name.clone()).unwrap_or_default();
        self.system(&format!("{name} chose {} on {what}: ready up again", mode_name(&self.choice.mode)));
        self.sent.clear();
        true
    }
    /// The fridge opens: the round plays the map the leader chose, or with Random one of the mode's maps drawn now (not
    /// the one played last, while the mode has another). On a map other than the one being played the game is switched
    /// (`switch`): the hotdog each seated member now plays is returned, by member id.
    fn start(&mut self) -> Option<BTreeMap<u32, u32>> {
        let mut entry = self.choice.clone();
        if entry.random {
            let mode = entry.mode.clone();
            self.catalogue();
            let all = self.pool(&mode);
            let fresh: Vec<Entry> = all.iter().filter(|e| Some(&e.name) != self.played.as_ref()).cloned().collect();
            let pool = if fresh.is_empty() { all } else { fresh };
            let roll = (self.draw)();
            entry = match pool.len() {
                0 => self.current.clone(),
                n => pool[((roll * n as f64).floor() as usize).min(n - 1)].clone(),
            };
        }
        let seats = (entry.name != self.current.name || entry.mode != self.current.mode).then(|| self.switch(entry));
        self.played = Some(self.current.name.clone());
        seats
    }
    /// A fresh game for `entry` takes over, the start sequence where it was, with every seated member on a hotdog of
    /// its own (in the order of their hotdogs before) with its name, colour, team asked for and ready mark. Round
    /// numbers go on rising, so that every client sees a new round.
    fn switch(&mut self, entry: Entry) -> BTreeMap<u32, u32> {
        let map = entry.map.as_deref().cloned().unwrap_or_else(Map::bundled);
        let mut next = Game::with_rules(map, &self.settings, rules_for(&entry));
        next.feast.round = self.game.feast.round;
        let old = std::mem::replace(&mut self.game, next);
        let mut seats = BTreeMap::new();
        let mut seated: Vec<(u32, u32)> = self.list().filter_map(|m| m.player.map(|p| (p, m.id))).collect();
        seated.sort();
        for (was, member) in seated {
            let ready = old.player(was).is_some_and(|p| p.ready);
            match self.game.join() {
                Some(p) => {
                    self.seat(member, p);
                    if ready {
                        self.game.ready(p, true);
                    }
                    seats.insert(member, p);
                }
                None => {
                    if let Some(m) = self.members.get_mut(&member) {
                        m.player = None;
                    }
                }
            }
        }
        self.game.open(old.opening);
        self.current = entry;
        self.sent.clear();
        seats
    }
    /// Forts: the side each seated member is on (a match is being played) or would start on (until it starts), by
    /// member id. None outside Forts.
    fn sides(&self) -> Option<BTreeMap<u32, u8>> {
        if !self.forts() {
            return None;
        }
        let mut seated: Vec<(u32, &Member)> = self.list().filter_map(|m| m.player.filter(|p| self.game.player(*p).is_some() && !self.game.waiting(*p)).map(|p| (p, m))).collect();
        seated.sort_by_key(|(p, _)| *p);
        Some(if matches!(self.game.phase, Phase::Lobby | Phase::Opening) || !self.game.forts() {
            muster(&seated.iter().map(|(_, m)| (m.id, m.team.as_str())).collect::<Vec<_>>(), FORT_PLAYERS / 2)
        } else {
            seated.iter().map(|(p, m)| (m.id, self.game.player(*p).map_or(0, |p| p.team))).collect()
        })
    }
    /// What the game sockets are told: the game's frame, with the hotdog the leader plays (`leader`, 0 for none),
    /// the next round's choice (`choice`, as in the roster) and, in Forts, the side of each seated player's hotdog
    /// (`sides`, by hotdog id), which the fridge shows.
    pub fn frame(&mut self) -> Frame {
        let leader = self.lead().and_then(|id| self.members.get(&id)).and_then(|m| m.player).unwrap_or(0);
        let sides = self.sides().map(|sides| {
            self.list().filter_map(|m| Some((m.player?.to_string(), *sides.get(&m.id)?))).collect::<BTreeMap<String, u8>>()
        });
        let mut frame = self.game.frame();
        (frame.leader, frame.choice, frame.sides) = (leader, self.choice.choice(), sides);
        frame
    }
    /// The frame as JSON: the debug encoding (a game socket that asks for it, and the tests).
    pub fn snapshot(&mut self) -> String {
        protocol::encode(&self.frame())
    }

    // ----- Members.

    /// A new member's profile: what was asked for where it is valid, and otherwise a name and a colour of its own.
    fn admit(&mut self, send: Option<Outbox>, asked: &Value, voice: bool) -> u32 {
        let id = self.next_member;
        self.next_member += 1;
        let order = (id - 1) as usize;
        let color = colour(&asked["color"], COLOURS[order % COLOURS.len()]);
        let wanted = clean_name(asked.get("name"));
        let taken = self.taken(None);
        let name = unique_name(if wanted.is_empty() { FALLBACK } else { &wanted }, taken.iter().map(String::as_str));
        let team = wish(&asked["team"]).unwrap_or("auto").to_string();
        let token = (self.token)();
        self.members.insert(id, Member { id, token, send, voice, player: None, name, color, mic: "none".into(), level: 0., spoke: 0., lines: vec![], team });
        id
    }
    /// A page's lobby socket says hello. Returns the member, or None when the lobby is full (the page is told why). A
    /// hello carrying the token of a member whose page was lost takes that member up again.
    pub fn enter(&mut self, send: Outbox, hello: &Value) -> Option<u32> {
        if let Some(token) = hello.get("token").and_then(Value::as_str)
            && let Some(old) = self.members.values_mut().find(|m| m.token == token && m.voice && m.send.is_none()) {
                old.send = Some(send);
                let id = old.id;
                self.welcome(id);
                self.sent.clear();
                return Some(id);
            }
        if self.full() {
            send.send_text(protocol::error(protocol::FULL));
            return None;
        }
        if self.game.phase == Phase::Lobby {
            self.catalogue();
        }
        let id = self.admit(Some(send), hello, true);
        self.welcome(id);
        let name = self.members[&id].name.clone();
        self.system(&format!("{name} joined"));
        self.sent.clear();
        Some(id)
    }
    fn welcome(&self, id: u32) {
        let Some(m) = self.members.get(&id) else {
            return;
        };
        self.tell(
            id,
            &json!({"type": "welcome", "id": id, "token": m.token, "colours": COLOURS, "limits": {"name": NAME, "chat": CHAT}, "history": self.history, "mode": self.mode(), "teams": TEAM_NAMES}),
        );
    }
    /// A game socket that came without a token (the native client): a member with no page, so no chat and no voice.
    /// None when the lobby is full.
    pub fn guest(&mut self, asked: &Value) -> Option<u32> {
        if self.full() {
            return None;
        }
        let id = self.admit(None, asked, false);
        let name = self.members[&id].name.clone();
        self.system(&format!("{name} joined"));
        Some(id)
    }
    /// The member a game socket's token belongs to, if it is not already playing.
    pub fn claim(&self, token: &Value) -> Option<u32> {
        let token = token.as_str()?;
        self.list().find(|m| m.token == token && m.player.is_none()).map(|m| m.id)
    }
    /// Ties a member to the hotdog its game socket was given: the game learns its name, colour and team.
    pub fn seat(&mut self, member: u32, player: u32) {
        let Some(m) = self.members.get_mut(&member) else {
            return;
        };
        m.player = Some(player);
        let (profile, team) = (Profile { name: m.name.clone(), color: m.color.clone() }, TEAMS.iter().position(|t| *t == m.team).map(|t| t as u8));
        self.game.profile(player, profile);
        self.game.team(player, team);
        self.sent.clear();
    }
    /// The member's game socket closed. A member with neither socket is gone.
    pub fn unseat(&mut self, member: u32) {
        let Some(m) = self.members.get_mut(&member) else {
            return;
        };
        m.player = None;
        if m.send.is_none() {
            self.remove(member);
        }
        self.sent.clear();
    }
    /// The member's lobby socket closed. While its game socket lives the member stays (its page may come back with
    /// its token).
    pub fn leave(&mut self, member: u32) {
        let Some(m) = self.members.get_mut(&member) else {
            return;
        };
        (m.send, m.mic, m.level) = (None, "none".into(), 0.);
        if m.player.is_none() {
            self.remove(member);
        }
        self.sent.clear();
    }
    /// What was said goes with the last to leave: nobody new is shown a conversation none of its speakers is still
    /// here for.
    pub fn remove(&mut self, member: u32) {
        if let Some(m) = self.members.remove(&member) {
            self.system(&format!("{} left", m.name));
        }
        if self.members.is_empty() {
            self.history.clear();
        }
    }
    fn tell(&self, member: u32, message: &Value) {
        if let Some(send) = self.members.get(&member).and_then(|m| m.send.as_ref()) {
            send.send_text(protocol::encode(message));
        }
    }
    fn broadcast(&self, message: &Value, except: Option<u32>) {
        let text = Utf8Bytes::from(protocol::encode(message));
        for m in self.list().filter(|m| Some(m.id) != except) {
            if let Some(send) = &m.send {
                send.send(&text);
            }
        }
    }
    fn system(&self, text: &str) {
        self.broadcast(&json!({"type": "system", "text": text}), None);
    }
    /// Changes a member's name or colour: only valid values are taken, the name is made unique, and everyone (and the
    /// game) hears of it.
    fn profile(&mut self, member: u32, asked: &Value) {
        let wanted = clean_name(asked.get("name"));
        let taken = self.taken(Some(member));
        let Some(m) = self.members.get_mut(&member) else {
            return;
        };
        let before = m.name.clone();
        if !wanted.is_empty() {
            m.name = unique_name(&wanted, taken.iter().map(String::as_str));
        }
        m.color = colour(&asked["color"], &m.color);
        let (name, profile, player) = (m.name.clone(), Profile { name: m.name.clone(), color: m.color.clone() }, m.player);
        if let Some(p) = player.filter(|p| self.game.player(*p).is_some()) {
            self.game.profile(p, profile);
        }
        if name != before {
            self.system(&format!("{before} is now {name}"));
        }
        self.sent.clear();
    }
    /// Forts: a member asks for a team, or for the server to choose. It counts for the next match: sides are made up
    /// when the round leaves the lobby.
    pub fn team(&mut self, member: u32, asked: &Value) -> bool {
        let Some(team) = wish(asked) else {
            return false;
        };
        if !self.forts() || self.members.get(&member).is_none_or(|m| m.team == team) {
            return false;
        }
        let m = self.members.get_mut(&member).unwrap();
        m.team = team.into();
        let (name, player) = (m.name.clone(), m.player);
        if let Some(p) = player {
            self.game.team(p, TEAMS.iter().position(|t| *t == team).map(|t| t as u8));
        }
        let text = match TEAMS.iter().position(|t| *t == team) {
            Some(t) => format!("{name} asks to join the {}", TEAM_NAMES[t]),
            None => format!("{name} will go where there is room"),
        };
        self.system(&text);
        self.sent.clear();
        true
    }
    /// A member says whether they are ready. Only somebody seated in the lobby can be (or in a Forts build phase).
    /// Returns whether it was taken.
    pub fn ready(&mut self, member: u32, flag: bool) -> bool {
        let Some(p) = self.player(member) else {
            self.tell(member, &json!({"type": "notice", "text": "Your hotdog is still on its way to the fridge."}));
            return false;
        };
        if self.game.waiting(p) {
            self.tell(member, &json!({"type": "notice", "text": "A round is in progress: you join the next one."}));
            return false;
        }
        if self.game.player(p).is_some_and(|p| p.ready == flag) || !self.game.ready(p, flag) {
            return false;
        }
        let name = self.members[&member].name.clone();
        self.system(&format!("{name} is {}", if flag { "ready" } else { "not ready" }));
        self.sent.clear();
        true
    }
    /// One message from a member's page. Anything malformed is ignored; nothing here can fail on bad input.
    pub fn handle(&mut self, member: u32, message: &Value) {
        if !message.is_object() || !self.members.contains_key(&member) {
            return;
        }
        match message.get("type").and_then(Value::as_str).unwrap_or("") {
            "profile" => self.profile(member, message),
            "ready" => {
                self.ready(member, message.get("ready") == Some(&Value::Bool(true)));
            }
            "team" => {
                self.team(member, &message["team"]);
            }
            "choose" => {
                self.choose(member, message.get("mode"), message.get("map"));
            }
            "chat" => self.chat(member, message),
            // Voice signalling goes from one member's page to another member's page and nowhere else.
            "signal" => {
                let to = message.get("to").and_then(protocol::integer).filter(|n| *n >= 0. && *n <= u32::MAX as f64).map(|n| n as u32);
                let (Some(to), Some(data)) = (to, signal(&message["data"])) else {
                    return;
                };
                let from = &self.members[&member];
                if let Some(target) = self.members.get(&to).filter(|t| t.id != member && t.send.is_some() && t.voice && from.voice) {
                    target.send.as_ref().unwrap().send_text(protocol::encode(&json!({"type": "signal", "from": member, "data": data})));
                }
            }
            "speaking" => {
                let level = protocol::numeric(&message["level"]).clamp(0., 1.);
                if self.members[&member].mic != "live" && level > 0. {
                    return;
                }
                self.voice(member, level);
            }
            "mic" => {
                let Some(state) = message.get("state").and_then(Value::as_str).filter(|s| ["none", "muted", "live"].contains(s)) else {
                    return;
                };
                let m = self.members.get_mut(&member).unwrap();
                if m.mic != state {
                    m.mic = state.into();
                    if state != "live" {
                        self.voice(member, 0.);
                    }
                    self.sent.clear();
                }
            }
            _ => {}
        }
    }
    fn chat(&mut self, member: u32, message: &Value) {
        let text = clean_chat(message.get("text"));
        if text.is_empty() {
            return;
        }
        let now = (self.now)();
        let m = self.members.get_mut(&member).unwrap();
        m.lines.retain(|t| now - t < RATE.1);
        if m.lines.len() >= RATE.0 {
            self.tell(member, &json!({"type": "notice", "text": "Slow down: a few lines at a time."}));
            return;
        }
        m.lines.push(now);
        let line = json!({"type": "chat", "from": member, "name": m.name, "color": m.color, "text": text});
        self.history.push(line.clone());
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.broadcast(&line, None);
    }
    fn voice(&mut self, member: u32, level: f64) {
        let rounded = (level * 20. + 0.5).floor() / 20.;
        let now = (self.now)();
        let Some(m) = self.members.get_mut(&member) else {
            return;
        };
        m.spoke = now;
        if rounded == m.level {
            return;
        }
        m.level = rounded;
        let player = m.player;
        self.broadcast(&json!({"type": "voice", "id": member, "level": rounded}), Some(member));
        if let Some(p) = player.filter(|p| self.game.player(*p).is_some()) {
            self.game.talk(p, rounded as f32);
        }
    }
    /// What every page shows: the stage, the countdown in whole seconds, and the members in the order they arrived.
    /// Forts adds the mode, the teams' names, the score, each member's wish (`team`) and the side it is on or would be
    /// on now (`side`: 0, 1, or null for somebody not seated), and the bots that make each side up to three (`bots`,
    /// by team). Every roster names the leader (`leader`, a member id, or null), the next round's choice (`choice`),
    /// the map being played (`playing`: what Random drew, once the fridge has opened) and the maps (`maps`).
    pub fn roster(&mut self) -> Value {
        let leader = self.lead();
        let forts = self.forts();
        let sides = self.sides();
        let side = |m: &Member| sides.as_ref().and_then(|s| s.get(&m.id).copied());
        let members: Vec<Value> = self
            .list()
            .map(|m| {
                let p = m.player.and_then(|p| self.game.player(p));
                let mut out = json!({"id": m.id, "name": m.name, "color": m.color, "player": m.player, "ready": p.is_some_and(|p| p.ready), "waiting": m.player.is_some_and(|p| self.game.waiting(p)), "voice": m.voice && m.send.is_some(), "mic": m.mic});
                if forts {
                    out["team"] = json!(m.team);
                    out["side"] = json!(side(m));
                }
                out
            })
            .collect();
        let mut roster = json!({
            "type": "roster",
            "stage": self.game.stage(),
            "countdown": self.game.countdown.map(|c| c.ceil().max(1.) as i64),
            "capacity": CAPACITY,
            "leader": leader,
            "choice": self.choice.shown(),
            "playing": self.current.shown(),
            "maps": self.maps.iter().map(Entry::listed).collect::<Vec<_>>(),
            "members": members,
        });
        if forts {
            let scores = self.game.feast.fort.as_ref().map_or([0, 0], |fort| fort.scores);
            let bots: Vec<i64> = (0..2u8).map(|t| (FORT_PLAYERS / 2) as i64 - self.list().filter(|m| side(m) == Some(t)).count() as i64).collect();
            roster["mode"] = json!("forts");
            roster["teams"] = json!(TEAM_NAMES);
            roster["scores"] = json!(scores);
            roster["bots"] = json!(bots);
        } else {
            roster["mode"] = json!(self.mode());
        }
        // The sandbox's rules, while it plays (the page shows them).
        if self.game.rules().sandbox {
            roster["rules"] = json!(self.game.rules().text());
        }
        roster
    }
    /// Called beside the game's tick: voices that have gone quiet fall silent, the leader is kept up to date, stage
    /// changes are announced, and the roster is sent when it has changed. When the fridge opens the round's map is
    /// settled (`start`) and the game switched to it if need be: then the hotdog each seated member plays now is
    /// returned, by member id. Back in the lobby the folder of maps is looked at again.
    pub fn tick(&mut self) -> Option<BTreeMap<u32, u32>> {
        let now = (self.now)();
        let quiet: Vec<u32> = self.list().filter(|m| m.level > 0. && now - m.spoke > QUIET).map(|m| m.id).collect();
        for id in quiet {
            self.voice(id, 0.);
        }
        self.lead();
        let stage = self.game.stage();
        let mut switched = None;
        if stage != self.stage {
            let message = match stage {
                "Opening" => Some("Everyone is ready: the fridge opens"),
                "Lobby" if self.stage != "Opening" => Some(if self.forts() { "Match over: back in the fridge" } else { "Round over: back in the fridge" }),
                "Fight" => Some("The forcefield is down"),
                "Build" if self.stage == "RoundOver" => Some("Next round: build"),
                _ => None,
            };
            if let Some(text) = message {
                self.system(text);
            }
            if stage == "Lobby" {
                self.catalogue();
            }
            self.stage = stage;
            if stage == "Opening" {
                switched = self.start();
            }
        }
        let roster = self.roster();
        let text = protocol::encode(&roster);
        if text != self.sent {
            self.broadcast(&roster, None);
            self.sent = text;
        }
        switched
    }
}
/// One of the five colours, or the fallback.
fn colour(value: &Value, fallback: &str) -> String {
    value.as_str().filter(|c| COLOURS.contains(c)).unwrap_or(fallback).to_string()
}
/// A team asked for ('auto', 'mustard' or 'ketchup'), or None for anything else.
fn wish(value: &Value) -> Option<&'static str> {
    let asked = value.as_str()?;
    WISHES.iter().find(|w| **w == asked).copied()
}
/// Whether a page message is one the lobby socket takes first.
pub fn hello(message: &Value) -> bool {
    is(message, "hello")
}

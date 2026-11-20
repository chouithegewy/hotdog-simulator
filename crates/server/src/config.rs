// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The command line.
//!
//!   --host H          the address to listen on (127.0.0.1; 0.0.0.0 for the network)
//!   --port P          the one port for everything (8137; 0 picks a free one)
//!   --map M           what the lobby starts on: a map's file (one from the editor; one that is not in assets/maps
//!                     can be chosen again under its file's name), a map's name (assets/maps/<name>.json), or random
//!   --mode siege|forts|ffa|cards|kungfu|titan  the mode the lobby starts on, which is the game's, not the map's
//!                     (kitchen_sim::rules `Mode`): on --map, or on the mode's default map (counter-siege, pillow-fort or
//!                     kung-fu-dojo), or with --map random the mode Random draws from. A --map that cannot be played in
//!                     the mode is refused with the reason: Forts needs the Forts layout, the dojo is Kung Fu's alone, and
//!                     `cards`, Fully Loaded (kitchen_sim::cards), is played on Counter Siege only (that, or random, are its
//!                     map). Without --mode the map's own mode
//!   --demo            no lobby: the bots play round after round for whoever watches
//!   --countdown S, --opening S   seconds the countdown and the start sequence last (3 and 9)
//!   --build S, --stale S         Forts: seconds the build phase lasts (150; a rule of the game, not the map's), and a
//!                     fight may stay quiet before the cats come (45); each 1 to 900
//!   --root DIR        the repository to serve (the working directory, or the one this was built in)
//!   --tick HZ         steps a second the game is simulated at (60), 20 to 240
//!   --send HZ         frames a second each client is sent (30), 1 to the steps a second
//!   --json            every game socket is sent its frames as JSON (the debug encoding; a socket can ask for it
//!                     itself in its join)
//!   --lag MS, --jitter MS   play over a slow network: every socket's messages (both ways) are held back by half of MS
//!                     each way, give or take half of the jitter, in order (for trying the netcode); 0 to 2000
//!   --ping S          every socket is sent a WebSocket ping every S seconds (20), and one that has answered none of the
//!                     last three (nor sent anything) is let go as a dead connection: this keeps a reverse proxy's idle
//!                     timeout (nginx's is 60 s) from cutting a lobby that is only sitting there; 0 sends none (0 to 900)
//!   --stats S         every S seconds, say what the game loop has cost and sent
//!   --dev             a game socket's join may ask for the dev sandbox (kitchen_sim::rules): the first to join an empty
//!                     game sets it up for everyone who follows, until the game is empty again; without --dev a join's
//!                     `sandbox` is passed over. In the sandbox a game socket may also change the tuning (`tune`)
//!   --bots easy|normal|hard   how well the bots aim in every game (kitchen_sim::aim; normal), unless a sandbox's
//!                     rules say otherwise (`skill=`)
//!   --tune "K=V ..."  the numbers every game starts with (kitchen_sim::tune: `mustard.speed=1.1,sniper.speed=12`; the
//!                     sandbox's panel copies them in this form)
//!   --replays DIR     where every match is recorded (replay.rs; `replays` in the root by default), listed at /replays
//!   --replays-keep N  how many recordings the folder keeps, the oldest deleted first (50); 0 records nothing
use crate::game::{COUNTDOWN, OPENING, Settings};
use crate::lobby::{self, RANDOM, default_map};
use kitchen_sim::map::Map;
use kitchen_sim::rules::Mode;
use std::path::{Path, PathBuf};

pub const PORT: u16 = 8137;

/// How the game is stepped and sent (see the module's documentation).
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub tick: u32,
    pub send: u32,
    pub json: bool,
    /// The round trip and its jitter added to every socket.
    pub lag: std::time::Duration,
    pub jitter: std::time::Duration,
    pub stats: Option<std::time::Duration>,
    /// How often each socket is pinged (transport.rs); zero: never.
    pub ping: std::time::Duration,
}
impl Default for Net {
    fn default() -> Net {
        Net { tick: kitchen_sim::net::TICK_HZ, send: kitchen_sim::net::SEND_HZ, json: false, lag: Default::default(), jitter: Default::default(), stats: None, ping: crate::transport::PING }
    }
}
impl Net {
    /// Steps between two frames.
    pub fn every(&self) -> u32 {
        (self.tick as f32 / self.send as f32).round().max(1.) as u32
    }
}

#[derive(Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub root: PathBuf,
    pub settings: Settings,
    /// The map the lobby starts on, the name it goes by when it is not one of the folder's, and whether the lobby
    /// starts on Random.
    pub map: Map,
    pub file: Option<String>,
    pub choice: Option<String>,
    pub random: bool,
    /// The mode the lobby starts on (--mode: siege, forts, ffa, cards, kungfu or titan), None for the map's own.
    pub mode: Option<String>,
    pub net: Net,
    /// The replays folder and how many it keeps (0: none recorded).
    pub replays: PathBuf,
    pub keep: usize,
}
/// The repository the server serves: the working directory when it holds the game, or else the folder of a package
/// (tools/build-*.sh) the server was unpacked with, or else the one this was built in.
pub fn default_root() -> PathBuf {
    let here = std::env::current_dir().unwrap_or_default();
    if here.join("assets/maps").is_dir() && here.join("index.html").is_file() {
        return here;
    }
    if let Some(packaged) = packaged_root() {
        return packaged;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap_or(here)
}
/// A package's folder: assets/maps beside the program, or beside the bin/ it is in (a Linux or Windows package, whose
/// launcher starts it), or in a macOS app's Contents/Resources.
fn packaged_root() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for root in [Some(dir.as_path()), dir.parent()].into_iter().flatten() {
        if root.join("assets/maps").is_dir() {
            return Some(root.to_path_buf());
        }
    }
    // (Not canonicalized above: on Windows that gives a \\?\ path, in which the "/" of "assets/maps" is no separator.)
    let resources = dir.join("../Resources").canonicalize().ok()?;
    resources.join("assets/maps").is_dir().then_some(resources)
}
fn usage() -> &'static str {
    "hotdog-server [--host 127.0.0.1] [--port 8137] [--map <file>|<name>|random] [--mode siege|forts|ffa|cards|kungfu|titan] [--demo]\n              [--countdown S] [--opening S] [--build S] [--stale S] [--root DIR]\n              [--tick HZ] [--send HZ] [--json] [--lag MS] [--jitter MS] [--ping S] [--stats S] [--dev]\n              [--bots easy|normal|hard] [--tune \"mustard.speed=1.1 sniper.speed=12 ...\"]\n              [--replays DIR] [--replays-keep N]"
}
/// Reads the command line (without the program's name). An error says what is wrong, as the server prints it.
pub fn parse(args: &[String]) -> Result<Config, String> {
    let value = |key: &str| args.iter().position(|a| a == key).map(|i| args.get(i + 1).cloned());
    let has = |key: &str| args.iter().any(|a| a == key);
    const VALUED: [&str; 19] = ["--host", "--port", "--map", "--mode", "--countdown", "--opening", "--build", "--stale", "--root", "--tick", "--send", "--lag", "--jitter", "--ping", "--stats", "--tune", "--bots", "--replays", "--replays-keep"];
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if VALUED.contains(&arg.as_str()) {
            skip = true;
        } else if arg == "--help" || arg == "-h" {
            return Err(usage().into());
        } else if arg == "--ws-port" || arg == "--tcp-port" {
            return Err(format!("{arg} is gone: one port serves the page, the game and the lobby; use --port\n{}", usage()));
        } else if arg != "--demo" && arg != "--json" && arg != "--dev" {
            return Err(format!("Unknown option {arg}\n{}", usage()));
        }
    }
    let host = value("--host").flatten().unwrap_or_else(|| "127.0.0.1".into());
    let port = match value("--port") {
        None => PORT,
        Some(port) => port.and_then(|p| p.parse().ok()).ok_or("--port needs a port number")?,
    };
    let root = match value("--root") {
        None => default_root(),
        Some(root) => PathBuf::from(root.ok_or("--root needs a folder")?),
    };
    let mode = match value("--mode") {
        None => None,
        Some(mode) => Some(mode.filter(|m| lobby::PLAYS.contains(&m.as_str())).ok_or("--mode needs siege or forts, ffa for Free For All, kungfu for Kung Fu, titan for Titan, or cards for Fully Loaded")?),
    };
    let cards = mode.as_deref() == Some(lobby::CARDS);
    let seconds = |key: &str, low: f32| -> Result<Option<f32>, String> {
        match value(key) {
            None => Ok(None),
            Some(text) => text.and_then(|t| t.parse::<f32>().ok()).filter(|s| *s >= low && *s <= 900.).map(Some).ok_or_else(|| {
                if low > 0. { format!("{key} needs a number of seconds from 1 to 900") } else { format!("{key} needs a number of seconds from 0 to 900") }
            }),
        }
    };
    let settings = Settings {
        demo: has("--demo"),
        countdown: seconds("--countdown", 0.)?.unwrap_or(COUNTDOWN),
        opening: seconds("--opening", 0.)?.unwrap_or(OPENING),
        build: seconds("--build", 1.)?,
        stale: seconds("--stale", 1.)?,
        dev: has("--dev"),
        skill: match value("--bots") {
            None => Default::default(),
            Some(skill) => skill.as_deref().and_then(kitchen_sim::aim::Skill::named).filter(|s| *s != kitchen_sim::aim::Skill::Perfect).ok_or("--bots needs easy, normal or hard")?,
        },
        tune: match value("--tune") {
            None => String::new(),
            Some(text) => {
                let text = text.ok_or("--tune needs a list of key=value, such as mustard.speed=1.1,sniper.speed=12")?;
                let mut tuning = kitchen_sim::tune::Tuning::default();
                let unknown = tuning.apply(&text);
                if !unknown.is_empty() {
                    return Err(format!("--tune does not know {}: keys are <gun>.<field> (mustard.speed, sniper.delay, relish.splash...), stim.* and move.*", unknown.join(", ")));
                }
                tuning.text()
            }
        },
    };
    let number = |key: &str, low: u32, high: u32| -> Result<Option<u32>, String> {
        match value(key) {
            None => Ok(None),
            Some(text) => text.and_then(|t| t.parse::<u32>().ok()).filter(|n| (low..=high).contains(n)).map(Some).ok_or_else(|| format!("{key} needs a whole number from {low} to {high}")),
        }
    };
    let tick = number("--tick", 20, 240)?.unwrap_or(kitchen_sim::net::TICK_HZ);
    let ms = |n: Option<u32>| std::time::Duration::from_millis(n.unwrap_or(0) as u64);
    let net = Net {
        tick,
        send: number("--send", 1, tick)?.unwrap_or(kitchen_sim::net::SEND_HZ.min(tick)),
        json: has("--json"),
        lag: ms(number("--lag", 0, 2000)?),
        jitter: ms(number("--jitter", 0, 2000)?),
        stats: number("--stats", 1, 3600)?.map(|s| std::time::Duration::from_secs(s as u64)),
        ping: seconds("--ping", 0.)?.map_or(crate::transport::PING, std::time::Duration::from_secs_f32),
    };
    // What the lobby starts on.
    let file = value("--map");
    let named = file.clone().flatten();
    let random = named.as_deref() == Some(RANDOM);
    let cannot = |reason: String| format!("Cannot play {}: {reason}", named.as_deref().unwrap_or("the bundled map"));
    if file.as_ref().is_some_and(Option::is_none) {
        return Err(cannot("--map needs a map file, a map's name or random".into()));
    }
    let maps = root.join("assets/maps");
    let fallback = match &mode {
        Some(mode) => {
            // (The mode's default map, else the first that can hold it; Fully Loaded's is Counter Siege.)
            let mut maps: Vec<lobby::Entry> = lobby::scan(&maps).maps.into_iter().filter(|e| lobby::refusal(e, mode).is_none()).collect();
            maps.sort_by_key(|e| e.name != default_map(mode));
            let what = if cards { default_map(mode).to_string() } else { format!("{mode} map") };
            Some(maps.into_iter().next().ok_or_else(|| cannot(format!("there is no {what} in assets/maps")))?)
        }
        None => None,
    };
    let map = match (&named, random) {
        (Some(name), false) => {
            // A bare word that is not a file is a map of assets/maps.
            let path = if lobby::map_name(name) && !Path::new(name).exists() { maps.join(format!("{name}.json")) } else { PathBuf::from(name) };
            let text = std::fs::read_to_string(&path).map_err(|error| cannot(format!("cannot read {}: {error}", path.display())))?;
            Map::parse(&text).map_err(|error| cannot(error.0))?
        }
        _ => match &fallback {
            Some(entry) => entry.map.as_deref().cloned().unwrap_or_else(Map::bundled),
            None => {
                let path = maps.join("counter-siege.json");
                match std::fs::read_to_string(&path) {
                    Ok(text) => Map::parse(&text).map_err(|error| cannot(error.0))?,
                    Err(_) => Map::bundled(),
                }
            }
        },
    };
    if let Some(mode) = &mode {
        if cards {
            if fallback.as_ref().is_some_and(|table| *table.text != *map.to_text()) {
                return Err(cannot("Fully Loaded is played on Counter Siege".into()));
            }
        } else if let Some(why) = Mode::named(mode).and_then(|m| map.refuses(m)) {
            return Err(cannot(format!("{why}, so it cannot be played as {}", lobby::mode_name(mode))));
        }
    }
    let choice = named.as_ref().filter(|_| !random).map(|name| {
        let file = Path::new(name).file_name().map_or(name.clone(), |file| file.to_string_lossy().to_string());
        file.strip_suffix(".json").map(str::to_string).unwrap_or(file)
    });
    let replays = match value("--replays") {
        None => root.join("replays"),
        Some(dir) => PathBuf::from(dir.ok_or("--replays needs a folder")?),
    };
    let keep = number("--replays-keep", 0, 100_000)?.map_or(crate::replay::KEEP, |n| n as usize);
    // (Fully Loaded has no Random: its one map is what the lobby starts on.)
    Ok(Config { host, port, root, settings, map, file: named.filter(|_| !random), choice, random: random && !cards, mode, net, replays, keep })
}

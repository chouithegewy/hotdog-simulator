// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Hotdog Simulator's server: one port for the page, the map editor, the assets, the game's WebSocket and each page's
//! lobby WebSocket, and the status.
//!
//! - `transport`: HTTP and WebSocket (axum): the router, each socket's task, its limits.
//! - `statics`: the files, read-only and only those the game and the editor load.
//! - `hub`: the game loop, which owns the lobby, its game and every socket's place in them.
//! - `lobby`: members, names, colours, chat, voice signalling, the leader and the next game.
//! - `game`: one map's game: kitchen-sim's simulation with the lobby's stages, seats, commands and frames.
//! - `protocol`: the messages (protocol 12: JSON text for the handshakes and the lobby, binary commands and frames
//!   for the game, kitchen_sim::net).
//! - `replay`: every match recorded for watching again, listed at /replays and served at /replays/<id>.
//! - `text`, `outbox`, `config`: names and chat lines, each socket's way out, the command line.
pub mod config;
pub mod game;
pub mod hub;
pub mod lobby;
pub mod outbox;
pub mod protocol;
pub mod replay;
pub mod statics;
pub mod text;
pub mod transport;
#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use tokio::net::TcpListener;

/// A server that is listening.
pub struct Running {
    pub addr: SocketAddr,
    pub task: tokio::task::JoinHandle<()>,
}
/// Builds the lobby and its game for `config`, listens, and runs until the process ends. `log` prints what the server
/// starts on.
pub async fn start(config: config::Config, log: bool) -> std::io::Result<Running> {
    let config::Config { host, port, root, settings, map, file, choice, random, mode, net, replays, keep } = config;
    // (The mode --mode asked for, on the map: its rules, naming no mode where it is the map's own.)
    let rules = mode.as_deref().and_then(kitchen_sim::rules::Mode::named).map_or_else(kitchen_sim::rules::Rules::default, |m| kitchen_sim::rules::Rules::playing(m, &map));
    let game = game::Game::with_rules(map, &settings, rules);
    let mut options = lobby::Options::new(root.join("assets/maps"));
    (options.choice, options.random, options.chosen) = (choice, random, mode.is_some());
    let lobby = lobby::Lobby::new(game, settings.clone(), options);
    if log {
        let game = &lobby.game;
        let fort = game.feast.fort.as_ref().map_or(String::new(), |fort| format!(", Forts: three a side, {} s to build", fort.build));
        println!("Map: {}{}{fort}", game.map.name, file.as_ref().map_or(String::new(), |file| format!(" ({file})")));
        let count = |mode: &str| lobby.maps.iter().filter(|e| lobby::refusal(e, mode).is_none()).count();
        let next = lobby.next_up();
        let skipped = if lobby.skipped.is_empty() { String::new() } else { format!("; not playable: {}", lobby.skipped.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")) };
        println!("The lobby's leader chooses the mode and the map apart, from {} maps (a siege or Free For All on {}, Forts on {}) or Random; next: {next}{skipped}", lobby.maps.len(), count("siege"), count("forts"));
    }
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    let addr = listener.local_addr()?;
    let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
    let app = transport::App::slowed(root, events, net.lag, net.jitter);
    let app = transport::App::with_replays(app, (keep > 0).then(|| replays.clone()));
    let app = transport::App::with_ping(app, net.ping);
    if log {
        let slow = if net.lag.is_zero() && net.jitter.is_zero() { String::new() } else { format!(", every socket slowed by {} ms ± {} ms round trip", net.lag.as_millis(), net.jitter.as_millis()) };
        println!("The game steps {} times a second and sends {} frames a second{}{slow}", net.tick, net.tick as f32 / net.every() as f32, if net.json { ", as JSON" } else { "" });
    }
    let mut hub = hub::Hub::new(lobby, net);
    hub.replays = replay::Replays::new(replays.clone(), keep);
    if log {
        if keep > 0 {
            println!("Every match is recorded to {} (the newest {keep} kept), listed at /replays", replays.display());
        } else {
            println!("Matches are not recorded (--replays-keep 0)");
        }
    }
    tokio::spawn(hub.run(receiver));
    if log {
        let shown = if host == "0.0.0.0" { "<this machine>".to_string() } else { addr.ip().to_string() };
        println!("Hotdog Simulator at http://{shown}:{}/ : play online at /?mp, the map editor at /editor/, the status at /status (sockets /ws and /lobby)", addr.port());
    }
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, transport::router(app)).await;
    });
    Ok(Running { addr, task })
}

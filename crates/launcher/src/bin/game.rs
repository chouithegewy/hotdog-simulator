// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The game's launcher, `hotdog-simulator` in a package (src/lib.rs). On Windows a window program, like the game, so that
//! a double-click opens no console.
#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    hotdog_launcher::launch("hotdog-simulator", true)
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The server's launcher, `hotdog-server` in a package (src/lib.rs): a console program, like the server.
fn main() {
    hotdog_launcher::launch("hotdog-server", false)
}

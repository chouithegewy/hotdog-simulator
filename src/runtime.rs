// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
use bevy::prelude::*;

#[derive(Resource, Default, Clone)]
pub struct Options {
    pub skip_intro: bool,
    pub profile: bool,
    pub profile_path: String,
    pub connect: String,
    pub uncapped: bool,
    pub demo: bool,
    /// Native `--map <file>`: a map in the editor's format to play instead of the bundled one.
    pub map: String,
    /// Native multiplayer, `--name` and `--color`: what this player is called and the colour its hotdog
    /// wears on the ring round its waist and its name tag (red, blue, green, yellow or black; `--lips` is the
    /// old name for `--color`). In a browser the page's lobby asks instead.
    pub name: String,
    pub color: String,
    /// Forts, single player and the demo: seconds the build phase lasts instead of what the map says
    /// (`--build <seconds>`, or `?build=` in a browser); 0 leaves it to the map.
    pub build: f32,
    /// Native multiplayer on a Forts server, `--team mustard` or `--team ketchup`: the team to ask for.
    pub team: String,
    /// Free For All and the dev sandbox (kitchen_sim::rules): the page's address as `key` and `key=value` words (in a
    /// browser), or native `--rules "sandbox weapons=mustard,sniper ..."`. Single player plays by them; online they
    /// go with the join (`sandbox`, which a --dev server takes) and the server's rules come back in its frames.
    pub params: String,
    /// Native `--tutorial`: give Fully Loaded's tutorial at the first table, whether or not it has been seen. In a
    /// browser `?tutorial` asks for it, a word of `params` (see [`Options::tutorial`]).
    pub tutorial: bool,
    /// A replay to watch (replay/): natively `--replay <file>`, in a browser `?replay=<id, url or local:name>`. The
    /// game is then the replay's viewer: watched (`demo`), fed from the file as from a server (`connect` is `replay:`).
    pub replay: String,
    /// Single player records its rounds (replay/record.rs) unless this is off: natively `--no-record`, or
    /// `HOTDOG_NO_RECORD` set (anything but `0`), or a scripted screenshot run (`HOTDOG_SHOT_DIR` set) without
    /// `--replays-dir`; in a browser `?norecord`. Tests, screenshots and scratch runs pass one of them, so that they
    /// never write into the player's own folder.
    pub record: bool,
    /// This player's own view is kept for replays (replay/pov.rs: single player's in its recording; online a copy
    /// beside the replays, which `record` governs, and sent to the server when the REPLAYS page's switch says so)
    /// unless this is off: natively `--no-pov` or `HOTDOG_NO_POV` set (anything but `0`); in a browser `?nopov`.
    pub pov: bool,
    /// Natively, where single player's recordings go and the replays list reads (instead of `replays/` beside the
    /// settings file): `--replays-dir <dir>`, or `HOTDOG_REPLAYS_DIR` (`HOTDOG_REPLAYS` is the same).
    pub replays_dir: String,
    /// In a browser `?check`: the page may read the game's state (`window.hotdogState`, telemetry.rs) and test hooks.
    pub check: bool,
}
/// What `Options::connect` is while a replay is watched: no server, the file's stream instead (network.rs).
pub const REPLAY: &str = "replay:";
impl Options {
    pub fn read() -> Self {
        let mut options = Self::from_platform();
        if !options.replay.is_empty() {
            (options.demo, options.skip_intro, options.connect) = (true, true, REPLAY.into());
        }
        options
    }
    fn from_platform() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let args: Vec<String> = std::env::args().collect();
            let value = |key: &str| {
                args.iter()
                    .position(|s| s == key)
                    .and_then(|i| args.get(i + 1))
                    .cloned()
                    .unwrap_or_default()
            };
            let demo = args.iter().any(|s| s == "--demo");
            Self {
                // Multiplayer starts in the lobby, never with the breakfast scene.
                skip_intro: demo || args.iter().any(|s| s == "--skip-intro") || args.iter().any(|s| s == "--connect"),
                profile: args.iter().any(|s| s == "--profile"),
                profile_path: value("--profile"),
                connect: value("--connect"),
                uncapped: args.iter().any(|s| s == "--uncapped"),
                demo,
                map: value("--map"),
                name: value("--name"),
                color: Some(value("--color")).filter(|color| !color.is_empty()).unwrap_or_else(|| value("--lips")),
                build: value("--build").parse().unwrap_or(0.),
                team: value("--team"),
                params: value("--rules"),
                tutorial: args.iter().any(|s| s == "--tutorial"),
                replay: value("--replay"),
                record: {
                    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
                    let off = args.iter().any(|s| s == "--no-record") || var("HOTDOG_NO_RECORD").is_some_and(|v| v != "0");
                    let scripted = var("HOTDOG_SHOT_DIR").is_some() && value("--replays-dir").is_empty();
                    !off && !scripted
                },
                pov: !args.iter().any(|s| s == "--no-pov") && std::env::var("HOTDOG_NO_POV").ok().filter(|v| !v.is_empty()).is_none_or(|v| v == "0"),
                replays_dir: Some(value("--replays-dir"))
                    .filter(|d| !d.is_empty())
                    .or_else(|| std::env::var("HOTDOG_REPLAYS_DIR").ok().filter(|v| !v.is_empty()))
                    .or_else(|| std::env::var("HOTDOG_REPLAYS").ok().filter(|v| !v.is_empty()))
                    .unwrap_or_default(),
                check: false,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let raw = browser_options();
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            Self {
                skip_intro: v["skip"].as_bool().unwrap_or(false),
                demo: v["demo"].as_bool().unwrap_or(false),
                profile: v["profile"].as_bool().unwrap_or(false),
                uncapped: v["uncapped"].as_bool().unwrap_or(false),
                connect: v["connect"].as_str().unwrap_or("").into(),
                build: v["build"].as_f64().unwrap_or(0.) as f32,
                params: v["params"].as_str().unwrap_or("").into(),
                replay: v["replay"].as_str().unwrap_or("").into(),
                record: v["record"].as_bool().unwrap_or(true),
                pov: v["pov"].as_bool().unwrap_or(true),
                check: v["check"].as_bool().unwrap_or(false),
                ..default()
            }
        }
    }
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function browser_options(){const q=new URLSearchParams(location.search);const intro=q.has('intro');return JSON.stringify({skip:!intro||q.has('play')||q.has('mp')||q.has('demo'),demo:q.has('demo'),profile:q.has('profile'),uncapped:q.has('uncapped'),connect:q.get('server')||(q.has('mp')?`${location.protocol==='https:'?'wss':'ws'}://${location.host}/ws`:''),build:Number(q.get('build'))||0,replay:q.get('replay')||'',record:!q.has('norecord'),pov:!q.has('nopov'),check:q.has('check'),params:[...q].map(([k,v])=>v?`${k}=${v}`:k).join(' ')});}
")]
extern "C" {
    fn browser_options() -> String;
}

impl Options {
    /// Whether Fully Loaded's tutorial is asked for: natively `--tutorial`, and `tutorial` among the address's words (in
    /// a browser `?tutorial`, natively a word of `--rules`).
    pub fn tutorial(&self) -> bool {
        self.tutorial || self.params.split([' ', '&']).any(|word| word.split('=').next() == Some("tutorial"))
    }
    /// The rules the address asks for (single player plays by them).
    pub fn rules(&self) -> kitchen_sim::rules::Rules {
        kitchen_sim::rules::Rules::parse(&self.params)
    }
    /// What a join asks the server for, when the address asks for the sandbox: its words as an object (the server
    /// reads it with the same rules; `mode` and `map` choose the map). None otherwise.
    pub fn sandbox(&self) -> Option<serde_json::Value> {
        const KEYS: [&str; 14] = ["sandbox", "weapons", "weapon", "ammo", "god", "bots", "botmode", "zombies", "field", "stim", "respawn", "mode", "map", "skill"];
        if !self.rules().sandbox {
            return None;
        }
        let mut asked = serde_json::Map::new();
        for word in self.params.split([' ', '&']).filter(|w| !w.is_empty()) {
            let (key, value) = word.split_once('=').unwrap_or((word, ""));
            if KEYS.contains(&key) {
                asked.insert(key.into(), serde_json::Value::String(value.into()));
            }
        }
        Some(serde_json::Value::Object(asked))
    }
}

pub fn asset_directory() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(exe) = std::env::current_exe() {
        let local = exe.parent().unwrap().join("assets");
        if local.is_dir() {
            return local.to_string_lossy().into();
        }
        // In a Linux or Windows package the programs are in bin/, beside the assets (tools/build-linux.sh).
        if let Some(packaged) = exe.parent().and_then(|bin| bin.parent()).map(|dir| dir.join("assets")).filter(|dir| dir.is_dir()) {
            return packaged.to_string_lossy().into();
        }
        // In the macOS app (tools/build-mac.sh) the program is in Contents/MacOS and the assets in Contents/Resources.
        #[cfg(target_os = "macos")]
        if let Some(bundled) = exe.parent().and_then(|dir| dir.join("../Resources/assets").canonicalize().ok()).filter(|dir| dir.is_dir()) {
            return bundled.to_string_lossy().into();
        }
    }
    "assets".into()
}

/// This build's version: the package's, and the commit when tools/build-*.sh set `HOTDOG_VERSION` (0.1.0+74c7c92).
pub const VERSION: &str = match option_env!("HOTDOG_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
/// The processor level this build was compiled for (tools/build-linux.sh, CPU).
pub const CPU_LEVEL: &str = if cfg!(target_feature = "avx2") {
    "x86-64-v3"
} else if cfg!(target_feature = "sse4.2") {
    "x86-64-v2"
} else {
    std::env::consts::ARCH
};
pub fn log_version() {
    // (A package's launcher says which build it chose, and why: crates/launcher.)
    #[cfg(not(target_arch = "wasm32"))]
    let chosen = std::env::var("HOTDOG_CPU_CHOSEN").map(|why| format!(", chosen by the launcher: {why}")).unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let chosen = "";
    info!("Hotdog Simulator {VERSION}, the {CPU_LEVEL} build{chosen}");
}

/// Stops the native game first thing, with a message instead of an illegal instruction, on a processor without the
/// x86-64 level the build was compiled for (tools/build-linux.sh and build-windows.sh, `CPU=v2` or `v3`). cpuid is read
/// directly, as `is_x86_feature_detected!` answers true, unasked, for any feature the build was compiled with. Plain
/// integer code, so that it runs on any x86-64 whatever the level (README.md, "Building and packaging").
#[cfg(target_arch = "x86_64")]
#[inline(never)]
pub fn check_cpu() {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    let (v2, v3) = (cfg!(target_feature = "sse4.2"), cfg!(target_feature = "avx2"));
    if !v2 && !v3 {
        return;
    }
    // Leaf 1 ECX: SSE3 0, SSSE3 9, CX16 13, SSE4.1 19, SSE4.2 20, POPCNT 23 (v2); FMA 12, MOVBE 22, XSAVE 26,
    // OSXSAVE 27, AVX 28, F16C 29 (v3). Leaf 7 EBX: BMI1 3, AVX2 5, BMI2 8. Leaf 0x80000001 ECX: LAHF 0, LZCNT 5.
    const V2: u32 = 1 | 1 << 9 | 1 << 13 | 1 << 19 | 1 << 20 | 1 << 23;
    const V3: u32 = V2 | 1 << 12 | 1 << 22 | 1 << 26 | 1 << 27 | 1 << 28 | 1 << 29;
    const V3_LEAF7: u32 = 1 << 3 | 1 << 5 | 1 << 8;
    #[allow(unused_unsafe)]
    let ok = unsafe {
        let (max, extended, ecx) = (__cpuid(0).eax, __cpuid(0x8000_0000).eax, __cpuid(1).ecx);
        let lahf = extended >= 0x8000_0001 && __cpuid(0x8000_0001).ecx & 1 != 0;
        if v3 {
            ecx & V3 == V3 && lahf && max >= 7 && __cpuid_count(7, 0).ebx & V3_LEAF7 == V3_LEAF7 && __cpuid(0x8000_0001).ecx & 1 << 5 != 0 && {
                // The system saves the SSE and AVX registers (XCR0 bits 1 and 2); OSXSAVE said xgetbv is there.
                let xcr0: u32;
                std::arch::asm!("xgetbv", in("ecx") 0, out("eax") xcr0, out("edx") _, options(nomem, nostack, preserves_flags));
                xcr0 & 6 == 6
            }
        } else {
            ecx & V2 == V2 && lahf
        }
    };
    if !ok {
        cpu_fail(v3);
    }
}
#[cfg(not(target_arch = "x86_64"))]
pub fn check_cpu() {}
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn cpu_fail(v3: bool) -> ! {
    use std::io::Write;
    let text: &[u8] = if v3 {
        b"This build of Hotdog Simulator is for x86-64-v3 processors (AVX2, FMA and BMI2: Intel from 2013, AMD from 2015), which this one is not. Start the game with hotdog-simulator beside the bin folder, which picks the build this processor runs.\0"
    } else {
        b"This build of Hotdog Simulator is for x86-64-v2 processors (SSE4.2, SSSE3 and POPCNT: from about 2009), which this one is not. Build the game for it with CPU=baseline (README.md, \"Building and packaging\").\0"
    };
    let _ = std::io::stderr().write_all(&text[..text.len() - 1]);
    let _ = std::io::stderr().write_all(b"\n");
    // (A release build on Windows has no console to print to.)
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxA(window: *mut std::ffi::c_void, text: *const u8, caption: *const u8, kind: u32) -> i32;
        }
        const MB_ICONERROR: u32 = 0x10;
        unsafe { MessageBoxA(std::ptr::null_mut(), text.as_ptr(), b"Hotdog Simulator\0".as_ptr(), MB_ICONERROR) };
    }
    std::process::exit(1);
}

pub fn software_lighting(
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut software: Local<Option<bool>>,
    mut points: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut directionals: Query<&mut DirectionalLight>,
) {
    let Some(adapter) = adapter else {
        return;
    };
    let software = *software.get_or_insert_with(|| {
        let cpu = format!("{:?}", adapter.device_type) == "Cpu";
        if cpu {
            info!("Software renderer: disabling shadow maps to keep the game responsive");
        }
        cpu
    });
    if !software {
        return;
    }
    for mut light in &mut points {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut spots {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut directionals {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
}

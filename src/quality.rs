// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Graphics that suit the device: a level chosen when the game starts, and adjusted while it plays.
//!
//! * **Levels.** Four, best first. [`Level::High`] is the look the build was made for (everything
//!   natively and on WebGPU, the WebGL 2 set in WebGL 2); `Medium`, `Low` and `Lowest` give up, in
//!   turn, the costs that buy least. Each is a [`Detail`]: the feature tier `visuals` dresses the scene
//!   for, the shadow map, anti-aliasing, HDR and bloom, fog, reflections, the film-grain overlay, the
//!   shadows of the kitchen's own lamps, how many rays, plants and lamps there are, how finely the
//!   hotdogs are made, and whether a picture drawn under the screen's resolution is sharpened.
//! * **Render scale.** In a browser the canvas can be drawn with fewer pixels than the screen has and
//!   stretched to fill the page: the page draws the canvas `--render-scale` of its size and magnifies
//!   it back (see `index.html`), so the browser hands the game a smaller drawing buffer, and the window's
//!   scale factor is overridden so that the head-up display keeps its size and the pointer its place.
//!   Each level has a range of scales, measured as pixels drawn per CSS pixel; the ladder of
//!   [`Rung`]s runs through every level's range in turn, best first. Natively the scale stays 1.
//! * **The start.** [`classify`] reads the GPU's name (WebGL's unmasked renderer, WebGPU's adapter
//!   info, wgpu's adapter name), whether it is a CPU renderer, the memory and processor hints and
//!   whether this is a phone; [`start`] maps that to a level and a pixel budget for the scale.
//! * **While playing.** With Auto chosen, the [`Governor`] watches the frame time and moves along the
//!   ladder: down when the game stays under 50 frames a second, up again with room to spare. The
//!   ladder gives up features before sharpness, and stops at a floor of 0.7 pixels per CSS pixel
//!   ([`FLOOR`]) with everything off: under it is a last resort, taken a step at a time and slowly, only
//!   while the game stays under 24 frames a second there, taken back when it does not help, and left as
//!   soon as there is room. It
//!   remembers where it settled, per device, in the browser's local storage, and the next visit
//!   starts there.
//! * **The player.** The quality menu on the page's frame-rate line (or F8) picks Auto or a level,
//!   and the page keeps the choice. `?quality=` (`--quality` natively) wins over both: `auto`, `high`,
//!   `medium`, `low`, `lowest`, and the old names `full` (High), `lite` (High with the WebGL 2 set)
//!   and `minimal` (Lowest).
//! * **The resolution.** The line's resolution menu picks Auto (the scale above) or a fixed share of
//!   the screen, 100% to 50% ([`Resolution`]), which holds whatever the level: under Auto quality the
//!   governor then moves between levels only, at that scale. `?scale=` (`80`, `80%` or `0.8`) wins over
//!   the menu. Natively there is no scale to fix.
//!
//! The frame-rate line on the page shows the level and the scale (`Auto · Low · 70%`).
use std::collections::VecDeque;

use bevy::{prelude::*, render::renderer::RenderAdapterInfo};

use crate::visuals::Quality;

/// The tier the build was made for: everything natively and with WebGPU, the WebGL 2 set otherwise.
pub fn best() -> Quality {
    if cfg!(all(target_arch = "wasm32", not(feature = "webgpu"))) {
        Quality::Lite
    } else {
        Quality::Full
    }
}

/// The four levels a player can pick, best first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Level {
    High,
    Medium,
    Low,
    Lowest,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::High, Level::Medium, Level::Low, Level::Lowest];

    pub fn name(self) -> &'static str {
        match self {
            Level::High => "High",
            Level::Medium => "Medium",
            Level::Low => "Low",
            Level::Lowest => "Lowest",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Level::High => "high",
            Level::Medium => "medium",
            Level::Low => "low",
            Level::Lowest => "lowest",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Level::ALL.into_iter().find(|level| level.key() == text.trim().to_ascii_lowercase())
    }
}

/// What the player asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Auto,
    Fixed(Level),
}

impl Choice {
    pub fn parse(text: &str) -> Option<Self> {
        if text.trim().eq_ignore_ascii_case("auto") {
            Some(Choice::Auto)
        } else {
            Level::parse(text).map(Choice::Fixed)
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn key(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::Fixed(level) => level.key(),
        }
    }

    /// F8 steps through them in the order the page lists them.
    fn next(self) -> Self {
        match self {
            Choice::Auto => Choice::Fixed(Level::High),
            Choice::Fixed(Level::High) => Choice::Fixed(Level::Medium),
            Choice::Fixed(Level::Medium) => Choice::Fixed(Level::Low),
            Choice::Fixed(Level::Low) => Choice::Fixed(Level::Lowest),
            Choice::Fixed(Level::Lowest) => Choice::Auto,
        }
    }
}

/// `?quality=` / `--quality`, which wins over the control.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Forced {
    pub choice: Choice,
    /// `lite`: High, but no more than the WebGL 2 set.
    pub lite: bool,
}

impl Forced {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Forced { choice: Choice::Fixed(Level::High), lite: false }),
            "lite" => Some(Forced { choice: Choice::Fixed(Level::High), lite: true }),
            "minimal" => Some(Forced { choice: Choice::Fixed(Level::Lowest), lite: false }),
            other => Choice::parse(other).map(|choice| Forced { choice, lite: false }),
        }
    }
}

/// What the player asked of the render scale: Auto (the governor's scale, or for a level picked by
/// hand the top of its range within its cap), or a fixed share of the screen's device pixels along
/// each axis, whatever the level.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Resolution {
    Auto,
    Fixed(f32),
}

impl Resolution {
    /// `auto`, or a share from a quarter to all of the screen: as a percentage (`80`, `80%`) or a
    /// fraction (`0.8`).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("auto") {
            return Some(Resolution::Auto);
        }
        let (number, percent) = match text.strip_suffix('%') {
            Some(number) => (number.trim(), true),
            None => (text, false),
        };
        let number: f32 = number.parse().ok()?;
        let share = if percent || number > 1. { number / 100. } else { number };
        (share.is_finite() && (0.25..=1.).contains(&share)).then_some(Resolution::Fixed(share))
    }

    /// As the page writes it: `auto`, or the percentage (`80`).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn key(self) -> String {
        match self {
            Resolution::Auto => "auto".into(),
            Resolution::Fixed(share) => format!("{}", (share * 100.).round() as u32),
        }
    }
}

/// Everything a level decides. `visuals` follows it whenever it changes.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct Detail {
    pub level: Level,
    /// The feature set the scene is dressed with.
    pub tier: Quality,
    /// The share of the screen's device pixels drawn along each axis (browser only; 1 natively).
    pub scale: f32,
    /// Texels along a side of the sun's shadow map; 0 for no shadows.
    pub shadow_map: u32,
    /// Gaussian filtering of the sun's shadows (otherwise one hardware-filtered tap).
    pub soft_shadows: bool,
    /// Whether the kitchen's own lamps (the ceiling spot, the fridge lamp) cast shadows.
    pub lamp_shadows: bool,
    /// Multisampling; without it the finished picture gets FXAA (`viewmodel::stack`).
    pub msaa: bool,
    /// A high-dynamic-range picture, with bloom.
    pub hdr: bool,
    pub fog: bool,
    /// The generated room reflected in shiny things.
    pub reflections: bool,
    /// Film grain and the vignette drawn over the picture.
    pub overlay: bool,
    /// Shares of the full count of light rays (and glints and motes) and of plants.
    pub rays: f32,
    pub plants: f32,
    /// Theme lamps that light the scene.
    pub lamps: usize,
    /// Hotdogs built with about a quarter of the triangles (the set a CPU renderer always has): their bodies,
    /// buns, mustard, eyes, limbs and faces (`player_model`).
    pub lean: bool,
    /// A picture drawn under the screen's resolution (a scale under 1) is sharpened before the page stretches
    /// it (contrast-adaptive sharpening), and its textures are sampled as finely as the screen would
    /// (a mip bias), `visuals`.
    pub sharpen: bool,
    /// FXAA where there is no multisampling (`viewmodel::stack`).
    pub fxaa: bool,
    /// The kitchen's own lamps over the counter (the ceiling spot, the fill light) lit in a round, where the theme
    /// has turned them down to a glimmer (`visuals::tune_room_lights`).
    pub room_lamps: bool,
}

impl Detail {
    /// A level's settings at full scale. `best` is the build's own tier.
    pub fn of(level: Level, best: Quality) -> Self {
        let high = Detail {
            level,
            tier: best,
            scale: 1.,
            shadow_map: 4096,
            soft_shadows: true,
            lamp_shadows: true,
            msaa: best != Quality::Full,
            hdr: true,
            fog: true,
            reflections: true,
            overlay: true,
            rays: 1.,
            plants: 1.,
            lamps: if best == Quality::Full { 12 } else { 10 },
            lean: false,
            sharpen: false,
            fxaa: true,
            room_lamps: true,
        };
        // Measured on an Arc B580 at 3840x2160 in WebGL 2 (README, Graphics): the theme's lamps, MSAA,
        // HDR with bloom and fog cost the most after the pixels themselves; reflections little (and
        // without them the room's ceiling goes dark, so only Lowest drops them); the shadow map's
        // size, the lamps' shadows, the rays and the film grain cost next to nothing there, and go
        // too where they are free to change, for weaker GPUs and processors.
        match level {
            Level::High => high,
            Level::Medium => Detail {
                tier: Quality::Lite,
                shadow_map: 2048,
                lamp_shadows: false,
                msaa: false,
                rays: 0.6,
                plants: 0.8,
                lamps: 4,
                sharpen: true,
                ..high
            },
            Level::Low => Detail {
                tier: Quality::Lite,
                shadow_map: 1024,
                soft_shadows: false,
                lamp_shadows: false,
                msaa: false,
                hdr: false,
                overlay: false,
                rays: 0.35,
                plants: 0.6,
                lamps: 2,
                lean: true,
                sharpen: true,
                ..high
            },
            Level::Lowest => Detail {
                tier: Quality::Minimal,
                shadow_map: 0,
                soft_shadows: false,
                lamp_shadows: false,
                msaa: false,
                hdr: false,
                fog: false,
                reflections: false,
                overlay: false,
                // A third of the minimal set's rays: hair-thin lines from the ceiling to the counter are cheap
                // on a GPU but cost a CPU renderer as much as the rest of the picture.
                rays: 0.35,
                plants: 0.,
                lamps: 0,
                room_lamps: false,
                lean: true,
                sharpen: true,
                ..high
            },
        }
    }

    /// What the frame-rate line says.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn label(&self, choice: Choice, forced: bool) -> String {
        let percent = (self.scale * 100.).round();
        let auto = if choice == Choice::Auto { "Auto · " } else { "" };
        let set = if forced { " (?quality)" } else { "" };
        format!("{auto}{} · {percent}%{set}", self.level.name())
    }
}

/// How finely each level may be drawn, as pixels drawn per CSS pixel along each axis: the most and
/// the least. On a 2x screen High comes down to about the page's own resolution before anything
/// else goes. Below that the levels give up their features before the picture gives up its sharpness:
/// Medium and Low keep 0.9 and 0.8, and Lowest stops at the [`FLOOR`].
fn densities(level: Level) -> (f32, f32) {
    match level {
        Level::High => (f32::INFINITY, 0.85),
        Level::Medium => (1.25, 0.9),
        Level::Low => (1.0, 0.8),
        Level::Lowest => (1.0, FLOOR),
    }
}

/// The fewest pixels per CSS pixel the scale comes to, along each axis, before every feature has gone: a
/// picture stretched further than this looks blurred. Under Auto only a game still slower than
/// [`LAST_RESORT_MS`] at Lowest on the floor goes under it, slowly, to [`LAST_RESORT`] at the most, and
/// comes back up as soon as there is room ([`Governor`]).
pub const FLOOR: f32 = 0.7;
/// The coarsest a last resort draws: half the page's own resolution.
pub const LAST_RESORT: f32 = 0.5;

/// The most pixels (millions) a level picked by the player is drawn with: a 4K screen at Low draws
/// as many as a 1080p one.
fn cap(level: Level) -> Option<f32> {
    match level {
        Level::High => None,
        Level::Medium => Some(3.7),
        Level::Low => Some(2.1),
        Level::Lowest => Some(1.2),
    }
}

/// The steps the scale moves in: each a little over a fifth fewer pixels than the one before.
const SCALES: [f32; 12] = [1.0, 0.9, 0.8, 0.7, 0.62, 0.55, 0.5, 0.45, 0.4, 0.35, 0.3, 0.25];

/// One place on the ladder: a level drawn at a scale (the share of the screen's device pixels along
/// each axis).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rung {
    pub level: Level,
    pub scale: f32,
}

/// The scales a level may be drawn at on a screen of `dpr` device pixels per CSS pixel, finest
/// first. Without scaling (natively), only 1.
fn scales(level: Level, dpr: f32, scalable: bool) -> Vec<f32> {
    if !scalable {
        return vec![1.];
    }
    let (most, least) = densities(level);
    let least = least.min(dpr);
    let mut scales: Vec<f32> =
        SCALES.into_iter().filter(|s| s * dpr <= most + 0.01 && s * dpr >= least - 0.01).collect();
    // Nothing in range (a very low or high density): the step nearest to it.
    if scales.is_empty() {
        let want = (most.min(dpr) / dpr).clamp(SCALES[SCALES.len() - 1], 1.);
        scales.push(SCALES.into_iter().min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs())).unwrap());
    }
    scales
}

/// The scales under the [`FLOOR`], down to the [`LAST_RESORT`], finest first: Lowest's last resort.
fn below_floor(dpr: f32, scalable: bool) -> Vec<f32> {
    if !scalable {
        return vec![];
    }
    let coarsest = *scales(Level::Lowest, dpr, scalable).last().unwrap();
    SCALES.into_iter().filter(|s| *s < coarsest - 0.001 && s * dpr >= LAST_RESORT - 0.01).collect()
}

/// The scales a level may be drawn at, with Lowest's last resort, finest first.
fn reach(level: Level, dpr: f32, scalable: bool) -> Vec<f32> {
    let mut mine = scales(level, dpr, scalable);
    if level == Level::Lowest {
        mine.extend(below_floor(dpr, scalable));
    }
    mine
}

/// The rung a level starts on: its finest scale whose pixels (`pixels`, the screen's in millions,
/// times the scale squared) fit `budget`. A screen with so many pixels that none does (4K at 1x) may
/// go coarser than the level's range, though never under the [`FLOOR`].
pub fn first(level: Level, dpr: f32, scalable: bool, pixels: f32, budget: Option<f32>) -> Rung {
    let mut mine = scales(level, dpr, scalable);
    let Some(budget) = budget else { return Rung { level, scale: mine[0] } };
    let coarsest = *mine.last().unwrap();
    if scalable {
        mine.extend(SCALES.into_iter().filter(|s| *s < coarsest - 0.001 && s * dpr >= FLOOR - 0.01));
    }
    let scale = mine.iter().copied().find(|s| pixels * s * s <= budget + 0.01).unwrap_or(*mine.last().unwrap());
    Rung { level, scale }
}

/// What a level picked by the player is drawn at: its finest scale within its cap, never under the floor.
pub fn picked(level: Level, dpr: f32, scalable: bool, pixels: f32) -> Rung {
    first(level, dpr, scalable, pixels, cap(level))
}

/// A level at the scale of its range nearest to `scale` (with `last_resort`, Lowest's last resort counts
/// as its range).
fn nearest(level: Level, scale: f32, dpr: f32, scalable: bool, last_resort: bool) -> Rung {
    let range = if last_resort { reach(level, dpr, scalable) } else { scales(level, dpr, scalable) };
    let scale = range.into_iter().min_by(|a, b| (a - scale).abs().total_cmp(&(b - scale).abs())).unwrap();
    Rung { level, scale }
}

/// What the governor moves over: the screen (device pixels per CSS pixel, and whether the scale can move at all: in a
/// browser), a scale the player fixed (then the levels alone move, at that scale), and whether the scale has been found
/// not to matter (`cpu_bound`: a game held back by its processor, where fewer pixels buy nothing; the levels alone move
/// then too, each drawn as finely as it may be, and only through those that cost the processor less, [`CPU`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shape {
    pub dpr: f32,
    pub scalable: bool,
    pub fixed: Option<f32>,
    pub cpu_bound: bool,
}

impl Shape {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(dpr: f32, scalable: bool) -> Self {
        Shape { dpr, scalable, fixed: None, cpu_bound: false }
    }
    /// Whether the scale moves along with the levels (in a browser, with no fixed resolution, while it is worth moving).
    fn scaled(self) -> bool {
        self.scalable && self.fixed.is_none() && !self.cpu_bound
    }
    /// The finest and the coarsest scale a level is drawn at while only the levels move.
    fn finest(self, level: Level) -> f32 {
        self.fixed.unwrap_or_else(|| scales(level, self.dpr, self.scalable)[0])
    }
    fn coarsest(self, level: Level) -> f32 {
        self.fixed.unwrap_or_else(|| *scales(level, self.dpr, self.scalable).last().unwrap())
    }
    /// Whether `rung` is under the floor: Lowest's last resort ([`FLOOR`], [`LAST_RESORT`]).
    pub fn under(self, rung: Rung) -> bool {
        self.scaled() && rung.level == Level::Lowest && rung.scale < self.coarsest(Level::Lowest) - 0.001
    }
    /// The levels the governor steps through: all of them, or with the processor as the limit those that cost it at
    /// least a twentieth less than the one above ([`CPU`], [`SAVES`]).
    fn levels(self) -> Vec<Level> {
        if !self.cpu_bound {
            return Level::ALL.to_vec();
        }
        let mut out = vec![Level::High];
        for level in Level::ALL.into_iter().skip(1) {
            if cpu(level) <= cpu(*out.last().unwrap()) * SAVES {
                out.push(level);
            }
        }
        out
    }
}

/// One step down from `rung`: features before sharpness. With the scale moving: the rest of the level's own range
/// (and at Lowest, under the floor, its last resort), then the next level at the finest of its scales no finer than
/// this. Otherwise the next level, at this scale as far as its range allows. None at the bottom.
pub fn down(rung: Rung, shape: Shape) -> Option<Rung> {
    if !shape.scaled() {
        let next = shape.levels().into_iter().find(|l| *l > rung.level)?;
        return Some(Rung { level: next, scale: rung.scale.clamp(shape.coarsest(next), shape.finest(next)) });
    }
    if let Some(scale) = reach(rung.level, shape.dpr, true).into_iter().find(|s| *s < rung.scale - 0.001) {
        return Some(Rung { level: rung.level, scale });
    }
    let next = Level::ALL.into_iter().find(|l| *l > rung.level)?;
    let scale = reach(next, shape.dpr, true).into_iter().find(|s| *s <= rung.scale + 0.001).unwrap_or(rung.scale);
    Some(Rung { level: next, scale })
}

/// One step up from `rung`: sharpness before features. With the scale moving: the next finer scale of the level's
/// own range (from the last resort up through the floor), then the next better level at the coarsest of its scales no
/// coarser than this. Otherwise the level's finest scale first, then the next better level. None at the top.
pub fn up(rung: Rung, shape: Shape) -> Option<Rung> {
    if !shape.scaled() {
        let finest = shape.finest(rung.level);
        if rung.scale < finest - 0.001 {
            return Some(Rung { level: rung.level, scale: finest });
        }
        let next = shape.levels().into_iter().rev().find(|l| *l < rung.level)?;
        return Some(Rung { level: next, scale: rung.scale.clamp(shape.coarsest(next), shape.finest(next)) });
    }
    if let Some(scale) = reach(rung.level, shape.dpr, true).into_iter().rev().find(|s| *s > rung.scale + 0.001) {
        return Some(Rung { level: rung.level, scale });
    }
    let next = Level::ALL.into_iter().rev().find(|l| *l < rung.level)?;
    let scale = scales(next, shape.dpr, true).into_iter().rev().find(|s| *s >= rung.scale - 0.001).unwrap_or(rung.scale);
    Some(Rung { level: next, scale })
}

/// Every rung the governor could step through from `anchor`, best first: the steps up from it and the steps down. So
/// every step down draws no more pixels and every step up no fewer. Returns the ladder and the anchor's place on it.
/// (The governor takes one step at a time, `up` and `down`; this is the whole of it, for the tests and to reason about.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn ladder(anchor: Rung, shape: Shape) -> (Vec<Rung>, usize) {
    let mut rungs = vec![];
    let mut at = anchor;
    while let Some(next) = up(at, shape) {
        rungs.push(next);
        at = next;
    }
    rungs.reverse();
    let place = rungs.len();
    rungs.push(anchor);
    let mut at = anchor;
    while let Some(next) = down(at, shape) {
        rungs.push(next);
        at = next;
    }
    (rungs, place)
}

/// What is known of the device when the game starts.
#[derive(Clone, Debug, Default)]
pub struct Device {
    /// Every name the GPU goes by, in lower case: WebGL's unmasked renderer and vendor, WebGPU's
    /// adapter vendor, architecture and description, and wgpu's adapter name.
    pub gpu: String,
    /// wgpu says it is a CPU renderer.
    pub software: bool,
    /// wgpu's word on the kind, natively, where it comes from the driver (in a browser wgpu guesses it
    /// from the name, and calls every Intel GPU integrated).
    pub integrated: bool,
    pub discrete: bool,
    /// `navigator.deviceMemory` in gigabytes and `navigator.hardwareConcurrency`; 0 when unknown.
    pub memory: f32,
    pub cores: u32,
    /// A phone or tablet.
    pub mobile: bool,
}

/// The kinds of GPU the start is decided by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// A CPU renderer: SwiftShader, llvmpipe, Microsoft's basic renderer.
    Software,
    /// A phone's or tablet's GPU.
    Phone,
    /// The integrated graphics of most laptops: Intel HD, UHD, Iris and Iris Xe; AMD's Vega and
    /// older Radeon Graphics.
    Integrated,
    /// The newer, larger integrated GPUs: Intel Arc graphics in Core Ultra, AMD Radeon 680M to 890M,
    /// Snapdragon X's Adreno.
    StrongIntegrated,
    /// Entry discrete GPUs no faster than the strong integrated ones: GeForce MX, GT 1030 and the like.
    EntryDiscrete,
    /// Apple silicon.
    Apple,
    Discrete,
    Unknown,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Software => "software renderer",
            Class::Phone => "phone or tablet",
            Class::Integrated => "integrated GPU",
            Class::StrongIntegrated => "strong integrated GPU",
            Class::EntryDiscrete => "entry discrete GPU",
            Class::Apple => "Apple GPU",
            Class::Discrete => "discrete GPU",
            Class::Unknown => "unknown GPU",
        }
    }
}

/// Whether `text` has `word` followed by a model number (`arc a770`, `arc(tm) b580`, `rx 6600`).
fn model_after(text: &str, word: &str, letters: &[char]) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let rest = text[at + word.len()..].trim_start_matches(|c: char| c == '(' || c == ')' || c == 't' || c == 'm' || c == ' ' || c == '®');
        let mut chars = rest.chars();
        let first = chars.next();
        let second = chars.next();
        match first {
            Some(c) if letters.contains(&c) => second.is_some_and(|d| d.is_ascii_digit()),
            Some(c) if letters.is_empty() => c.is_ascii_digit(),
            _ => false,
        }
    })
}

/// What kind of GPU this is.
pub fn classify(device: &Device) -> Class {
    let gpu = device.gpu.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| gpu.contains(w));
    if device.software
        || has(&["swiftshader", "llvmpipe", "lavapipe", "softpipe", "software", "basic render", "mesa offscreen"])
    {
        return Class::Software;
    }
    if device.mobile || has(&["mali", "powervr", "immortalis", "xclipse", "videocore"]) {
        return Class::Phone;
    }
    if has(&["adreno"]) {
        // Snapdragon X laptops; any other Adreno is a phone's.
        return if has(&["adreno x1", "x1-85", "x1-45", "x elite", "x plus"]) { Class::StrongIntegrated } else { Class::Phone };
    }
    if has(&["apple"]) {
        return Class::Apple;
    }
    if has(&["nvidia", "geforce", "quadro", "rtx", "gtx", "tesla", "titan"]) {
        if has(&["geforce mx", " mx1", " mx2", " mx3", " mx4", " mx5", "gt 1030", "gt 730", "gt 720", "gt 710", "geforce 9", "geforce 8"]) {
            return Class::EntryDiscrete;
        }
        return Class::Discrete;
    }
    if has(&["radeon", "amd", "ati technologies"]) {
        if model_after(&gpu, "rx", &[]) || has(&["radeon pro", "firepro", "radeon vii", "vega 56", "vega 64", "r9 ", "rx vega", "navi"]) {
            return Class::Discrete;
        }
        if has(&["680m", "660m", "740m", "760m", "780m", "880m", "890m", "8040s", "8050s", "8060s", "rembrandt", "phoenix", "hawk point", "strix", "krackan", "gfx1103", "gfx1150", "gfx1151"]) {
            return Class::StrongIntegrated;
        }
        if has(&["radeon 5", "radeon 6", "r7 m", "r5 m", "hd 8", "hd 7"]) && !has(&["graphics"]) {
            return Class::EntryDiscrete;
        }
        return Class::Integrated;
    }
    if has(&["intel"]) {
        // Arc with a model number (A380, A770M, B580) is a graphics card; plain "Arc Graphics" (and
        // Arc 130V/140V) is the integrated GPU of a Core Ultra.
        // WebGPU names the architecture instead: xe-hpg and xe-2hpg are the cards, xe-lpg and xe-2lpg
        // Core Ultra's graphics, gen-12lp (Iris Xe) and older the rest.
        if model_after(&gpu, "arc", &['a', 'b']) || has(&["dg2", "bmg", "battlemage", "alchemist", "xe-hpg", "xe-2hpg"]) {
            return Class::Discrete;
        }
        if has(&["arc", "meteor lake", "lunar lake", "arrow lake", "mtl", "lnl", "arl", "xe-lpg", "xe-2lpg"]) {
            return Class::StrongIntegrated;
        }
        return Class::Integrated;
    }
    if device.integrated {
        return Class::Integrated;
    }
    if device.discrete {
        return Class::Discrete;
    }
    Class::Unknown
}

/// The level a class starts on, and a budget for the pixels it draws (millions), where the level's
/// finest scale would cost too much on a screen of many pixels.
pub fn start(class: Class, device: &Device) -> (Level, Option<f32>) {
    let (level, budget) = match class {
        Class::Software => (Level::Lowest, Some(0.6)),
        Class::Phone => (Level::Low, Some(1.0)),
        Class::Integrated => (Level::Low, Some(1.2)),
        Class::StrongIntegrated | Class::EntryDiscrete => (Level::Medium, Some(2.1)),
        Class::Apple => (Level::High, Some(3.7)),
        Class::Discrete => (Level::High, None),
        Class::Unknown => {
            if (device.memory > 0. && device.memory <= 4.) || (device.cores > 0 && device.cores <= 4) {
                (Level::Low, Some(1.2))
            } else {
                (Level::Medium, Some(2.1))
            }
        }
    };
    // Very little memory says a weak machine, whatever the GPU's name.
    if device.memory > 0. && device.memory <= 2. && level < Level::Low {
        return (Level::Low, Some(1.2));
    }
    (level, budget)
}

/// The best a replay's viewer may draw at ([`Graphics::cap`]): on a phone or tablet, the device's own start; elsewhere
/// no limit.
pub fn viewer_cap(class: Class, replay: bool, start: Rung) -> Option<Rung> {
    (replay && class == Class::Phone).then_some(start)
}

/// The frame rate the governor keeps the game above, as a frame time: slower than this for a while is too slow.
pub const TARGET_MS: f32 = 20.;
/// A window whose pace is under this holds a display's 60 Hz: a frame rate that reads 56 to 64 around a 60 Hz cap is
/// holding it. A step up is kept only where the game still holds this afterwards.
pub const HOLD_MS: f32 = 18.5;
/// The frame times judged at once, in seconds.
const WINDOW: f64 = 2.5;
/// How often the governor judges.
const LOOK: f64 = 0.5;
/// How long the game must stay too slow before a step down.
const SLOW_FOR: f64 = 3.5;
/// How long there must be room before a step up. Where the main thread's work is measured (a browser), room is the game
/// holding the display's rate with that work leaving space for the step: UP_ROOM_FOR before a finer scale, and
/// UP_LEVEL_ROOM_FOR before a better level (which recompiles shaders) whose cost to the main thread ([`CPU`]) fits.
/// Where it is not (natively): UP_FOR for a finer scale; for a better level UP_LEVEL_FOR with plenty of room (the frame
/// time well under the target) or, where a display's 60 Hz hides how much room there is, UP_LEVEL_CAPPED_FOR held at it.
const UP_ROOM_FOR: f64 = 4.;
const UP_LEVEL_ROOM_FOR: f64 = 6.;
const UP_FOR: f64 = 8.;
const UP_LEVEL_FOR: f64 = 15.;
const UP_LEVEL_CAPPED_FOR: f64 = 40.;
/// Below this share of the target, there is plenty of room.
const ROOMY: f32 = 0.7;
/// The main thread's work as a share of the frame: at BUSY_FULL or more the processor is what holds the game back (a
/// GPU-bound frame leaves the main thread idle most of it; a browser's own work between frames takes some), and
/// fewer pixels cannot help; a better level is tried only where its work, as [`CPU`] predicts it, stays within BUSY_FIT
/// of the frame (of a 60 Hz frame at least).
const BUSY_FULL: f32 = 0.75;
const BUSY_FIT: f32 = 0.8;
/// Under this share of a slow frame the main thread was mostly idle: the frame waited on the GPU.
const BUSY_IDLE: f32 = 0.6;
const FRAME_60: f32 = 1000. / 60.;
/// Frame times ignored at the start (shaders compiling, assets arriving), after a step of scale, and
/// after a change of level.
const QUIET_START: f64 = 10.;
const QUIET_SCALE: f64 = 1.5;
const QUIET_LEVEL: f64 = 4.;
/// A frame longer than this is not the game's pace (the tab was hidden, the machine slept): start
/// over a little later.
const GAP_MS: f32 = 600.;
/// A step up is judged on the first full window after it: it has to hold (the display's rate, or a tenth over what the
/// rung before managed). One that does not, or is too slow within PROBE seconds, is taken back, and that rung is not
/// tried again for BACKOFF seconds after its first failure, twice as long after each one after, up to BACKOFF_MOST.
const PROBE: f64 = 25.;
const BACKOFF: f64 = 30.;
const BACKOFF_MOST: f64 = 1800.;
/// Held this long without a change, and smooth, the rung is settled and remembered.
const SETTLE: f64 = 15.;
/// Under the floor ([`FLOOR`], [`Shape::under`]): the frame rate a step further down has to be slower than,
/// 24 frames a second, as a frame time; how long it must stay that slow before each step, longer than
/// SLOW_FOR as each costs sharpness; and the share of it under which there is room for a step back up
/// (32 frames a second), held for UP_FOR.
pub const LAST_RESORT_MS: f32 = 1000. / 24.;
const LAST_RESORT_FOR: f64 = 8.;
const LAST_RESORT_ROOM: f32 = 0.75;
/// Every step of scale down has to make the frame at least this much shorter (a share of what it was). One that does
/// not (the game is held back by the processor, not by the pixels) is taken back at once, and from then on, for the
/// session, the scale is left alone ([`Shape::cpu_bound`]): the levels alone move, and the scale goes back up to the
/// finest the level is drawn at as soon as that is no slower.
const HELPED: f32 = 0.9;
/// What each level costs the main thread per frame, as a share of Lowest's (High, Medium, Low, Lowest). Measured in a
/// browser (WebGL 2, 1280x720, the processor slowed 4x; Free For All and Pillow Fort online, the Kung Fu dojo), the most
/// of the three scenes: High 1.78–1.91, Medium 1.49–1.63, Low 1.19–1.29 (`.superpowers/sdd/adhoc/cpu-report.md`). It
/// predicts whether a better level fits the main thread's room, and with the processor as the limit which levels are
/// worth a step down (each of these saves at least a fifth of the one above).
pub const CPU: [f32; 4] = [1.9, 1.6, 1.3, 1.];
/// A level is a step down for a game held back by its processor only if it costs it at most this share of the one above.
const SAVES: f32 = 0.95;

fn cpu(level: Level) -> f32 {
    CPU[level as usize]
}

/// What the governor knows of a rung it has tried: how often a step up to it failed, and until when it is not tried.
#[derive(Clone, Copy, Debug, Default)]
struct Memo {
    failures: u8,
    blocked: f64,
}

/// A step up being watched: to which rung, from which, when, whether its first full window has been judged, and the
/// pace it must keep (ms).
#[derive(Clone, Copy, Debug)]
struct Probe {
    rung: Rung,
    from: Rung,
    at: f64,
    judged: bool,
    bar: f32,
}

/// Watches the frame time (and, where it is measured, the main thread's work in each frame) and moves between rungs.
/// Pure: fed the time and each frame, it says which rung to move to.
#[derive(Clone, Debug)]
pub struct Governor {
    pub rung: Rung,
    pub shape: Shape,
    /// (time, frame ms, main-thread ms or NaN)
    samples: VecDeque<(f64, f32, f32)>,
    quiet_until: f64,
    next_look: f64,
    /// Seconds the verdict has held: too slow; held or roomy (spare); plenty of room (roomy); held with room on the
    /// main thread for the next step (room).
    slow: f64,
    spare: f64,
    roomy: f64,
    room: f64,
    memos: Vec<(Rung, Memo)>,
    probe: Option<Probe>,
    changed: f64,
    /// Whether the last window judged was fast enough.
    smooth: bool,
    /// The step of scale down last taken (to, from), and the pace and the main thread's work before it: it has to help
    /// ([`HELPED`]).
    dropped: Option<(Rung, Rung, f32, Option<f32>)>,
    /// The best it may go to ([`Graphics::cap`]: a replay watched on a phone keeps to the device's own start): it steps
    /// down as ever, and back up no further than this.
    pub cap: Option<Rung>,
}

/// Whether `rung` is better than `cap`: a better level, or the same level drawn more finely.
pub fn above(rung: Rung, cap: Rung) -> bool {
    rung.level < cap.level || (rung.level == cap.level && rung.scale > cap.scale + 0.001)
}
/// `rung`, or `cap` where `rung` is better than it.
pub fn within(rung: Rung, cap: Option<Rung>) -> Rung {
    match cap {
        Some(cap) if above(rung, cap) => cap,
        _ => rung,
    }
}

/// What the governor saw in its window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// The mean frame time without hitches: frames four times the median and more (a shader
    /// compiling, a texture decoding, a round starting), and the slowest twentieth of the rest. When
    /// the long frames take a quarter of the time or more they are not hitches but the pace itself
    /// (a browser that lets several frames through and then waits a long while for the GPU), and the
    /// plain mean is used.
    pub mean: f32,
    pub p90: f32,
}

pub fn pace(times: impl Iterator<Item = f32>) -> Pace {
    let mut times: Vec<f32> = times.collect();
    times.sort_by(f32::total_cmp);
    let total: f32 = times.iter().sum();
    let plain = total / times.len().max(1) as f32;
    let hitch = (times[times.len() / 2] * 4.).max(60.);
    let long: f32 = times.iter().filter(|t| **t >= hitch).sum();
    times.retain(|t| *t < hitch);
    let p90 = times.get(((times.len().max(1) - 1) as f32 * 0.9) as usize).copied().unwrap_or(plain);
    if long >= total * 0.25 || times.is_empty() {
        return Pace { mean: plain, p90: p90.max(plain) };
    }
    let kept = times.len() - times.len() / 20;
    let mean = times[..kept].iter().sum::<f32>() / kept.max(1) as f32;
    Pace { mean, p90 }
}

fn same(a: Rung, b: Rung) -> bool {
    a.level == b.level && (a.scale - b.scale).abs() < 0.001
}

impl Governor {
    /// Starts on `rung` at time `now` (seconds), moving over `shape`.
    pub fn new(rung: Rung, shape: Shape, now: f64) -> Self {
        Governor {
            rung,
            shape,
            samples: VecDeque::new(),
            quiet_until: now + QUIET_START,
            next_look: 0.,
            slow: 0.,
            spare: 0.,
            roomy: 0.,
            room: 0.,
            memos: vec![],
            probe: None,
            changed: now,
            smooth: false,
            dropped: None,
            cap: None,
        }
    }

    fn memo(&self, rung: Rung) -> Memo {
        self.memos.iter().find(|(r, _)| same(*r, rung)).map_or_else(Memo::default, |(_, m)| *m)
    }

    /// A step up to `rung` failed: not again for a while, longer each time.
    fn fail(&mut self, rung: Rung, now: f64) {
        let mut memo = self.memo(rung);
        memo.failures = memo.failures.saturating_add(1);
        memo.blocked = now + (BACKOFF * 2f64.powi(memo.failures as i32 - 1)).min(BACKOFF_MOST);
        match self.memos.iter_mut().find(|(r, _)| same(*r, rung)) {
            Some(entry) => entry.1 = memo,
            None => self.memos.push((rung, memo)),
        }
    }

    /// How often a step up to `rung` has failed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn failures(&self, rung: Rung) -> u8 {
        self.memo(rung).failures
    }

    /// Forgets what it saw and ignores the next `quiet` seconds.
    fn rest(&mut self, now: f64, quiet: f64) {
        self.samples.clear();
        self.quiet_until = self.quiet_until.max(now + quiet);
        self.slow = 0.;
        self.spare = 0.;
        self.roomy = 0.;
        self.room = 0.;
    }

    fn go(&mut self, rung: Rung, now: f64) -> Rung {
        let level = rung.level != self.rung.level;
        self.rung = rung;
        self.changed = now;
        self.rest(now, if level { QUIET_LEVEL } else { QUIET_SCALE });
        rung
    }

    /// Whether the rung has held long enough, smoothly, to be remembered.
    pub fn settled(&self, now: f64) -> bool {
        now - self.changed >= SETTLE && self.probe.is_none() && self.smooth
    }

    /// The main thread's work in the window, the median of the frames that measured it; None where too few did.
    fn busy(&self) -> Option<f32> {
        let mut busy: Vec<f32> = self.samples.iter().map(|s| s.2).filter(|b| b.is_finite()).collect();
        if busy.len() * 2 < self.samples.len() || busy.is_empty() {
            return None;
        }
        busy.sort_by(f32::total_cmp);
        Some(busy[busy.len() / 2])
    }

    /// One frame of `ms` milliseconds, ending at `now` seconds, of which the main thread worked `busy` milliseconds
    /// (None where that is not measured). Returns the rung to move to.
    pub fn frame(&mut self, now: f64, ms: f32, busy: Option<f32>) -> Option<Rung> {
        if !ms.is_finite() || ms > GAP_MS {
            self.rest(now, 2.);
            return None;
        }
        if now < self.quiet_until {
            return None;
        }
        self.samples.push_back((now, ms, busy.filter(|b| b.is_finite()).unwrap_or(f32::NAN)));
        while self.samples.front().is_some_and(|s| s.0 < now - WINDOW) {
            self.samples.pop_front();
        }
        if now < self.next_look {
            return None;
        }
        self.next_look = now + LOOK;
        // Judge only a window that is mostly full; a step's verdict only a full one.
        let span = self.samples.front().map_or(0., |s| now - s.0);
        if span < WINDOW * 0.6 {
            return None;
        }
        let full = span >= WINDOW * 0.9;
        let Pace { mean, .. } = pace(self.samples.iter().map(|s| s.1));
        let busy = self.busy();
        let held = mean <= HOLD_MS;
        if self.probe.is_some_and(|p| now - p.at > PROBE) {
            self.probe = None;
        }
        // A step up has to hold: back at once from one that does not.
        if let Some(probe) = self.probe.filter(|p| !p.judged && full && same(p.rung, self.rung)) {
            self.probe = Some(Probe { judged: true, ..probe });
            if mean > probe.bar {
                self.probe = None;
                self.fail(probe.rung, now);
                return Some(self.go(probe.from, now));
            }
        }
        // A step of scale down has to help. One that did not gave up sharpness for nothing: back, and the scale is left
        // to the levels from now on.
        if let Some((to, from, before, worked)) = self.dropped.filter(|_| full) {
            self.dropped = None;
            // (Where the main thread was idle most of the slow frame, the time went to the GPU, which the pixels drive: a
            // step that shows no gain there is the display's 60 Hz rounding the frame up to the same whole number of
            // refreshes, and the next step may well show one.)
            let gpu = worked.is_some_and(|w| w < BUSY_IDLE * before);
            if same(to, self.rung) && mean > before * HELPED && !gpu {
                self.shape.cpu_bound = true;
                return Some(self.go(from, now));
            }
        }
        let next = down(self.rung, self.shape);
        // On the floor and under it a step down is a last resort: only for a game slower than
        // LAST_RESORT_MS, and only after a longer while; under it, a step back up wants room under that.
        let (limit, slow_for) = if next.is_some_and(|n| self.shape.under(n)) { (LAST_RESORT_MS, LAST_RESORT_FOR) } else { (TARGET_MS, SLOW_FOR) };
        let slow = mean > limit;
        let roomy = mean < TARGET_MS * ROOMY;
        let spare = roomy || held || (self.shape.under(self.rung) && mean < LAST_RESORT_MS * LAST_RESORT_ROOM);
        self.smooth = !slow;
        self.slow = if slow { self.slow + LOOK } else { 0. };
        self.spare = if spare { self.spare + LOOK } else { 0. };
        self.roomy = if roomy { self.roomy + LOOK } else { 0. };
        if self.slow >= slow_for {
            if let Some(mut next) = next {
                // A step up that did not last.
                if let Some(probe) = self.probe.take().filter(|p| same(p.rung, self.rung)) {
                    self.fail(probe.rung, now);
                }
                if next.level == self.rung.level {
                    if busy.is_some_and(|b| b >= BUSY_FULL * mean) {
                        // The main thread is at work the whole frame: fewer pixels cannot help. The levels alone move.
                        self.shape.cpu_bound = true;
                        match down(self.rung, self.shape) {
                            Some(level) => next = level,
                            None => return None,
                        }
                    } else {
                        self.dropped = Some((next, self.rung, mean, busy));
                    }
                }
                return Some(self.go(next, now));
            }
        }
        let up = up(self.rung, self.shape)?;
        if self.cap.is_some_and(|cap| above(up, cap)) {
            return None;
        }
        let level = up.level != self.rung.level;
        // The main thread's room for the step: a better level costs it more ([`CPU`]), a finer scale next to nothing.
        let fits = busy.map(|b| !level || b * cpu(up.level) / cpu(self.rung.level) <= BUSY_FIT * mean.max(FRAME_60));
        self.room = if held && fits == Some(true) { self.room + LOOK } else { 0. };
        // With the processor as the limit, a coarser scale than the level's finest buys nothing: back up to it after a
        // while, and keep it if the game is no slower there.
        let unpixelled = self.shape.cpu_bound && !level && now - self.changed >= UP_FOR;
        let ready = if level {
            (self.roomy >= UP_LEVEL_FOR && fits != Some(false)) || self.room >= UP_LEVEL_ROOM_FOR || (fits.is_none() && self.spare >= UP_LEVEL_CAPPED_FOR)
        } else {
            self.room >= UP_ROOM_FOR || self.spare >= UP_FOR || unpixelled
        };
        if ready && now >= self.memo(up).blocked {
            // Under the floor the step only has to stay faster than the last resort's 24 frames a second.
            let bar = if self.shape.under(self.rung) {
                LAST_RESORT_MS
            } else if held || roomy {
                HOLD_MS.max(mean * 1.1)
            } else {
                mean * 1.1
            };
            self.probe = Some(Probe { rung: up, from: self.rung, at: now, judged: false, bar });
            return Some(self.go(up, now));
        }
        None
    }
}

/// The state of the graphics: what was asked for, and where the game is.
#[derive(Resource)]
pub struct Graphics {
    pub choice: Choice,
    pub forced: Option<Forced>,
    pub resolution: Resolution,
    /// `?scale=`, which wins over the page's resolution menu.
    pub forced_resolution: Option<Resolution>,
    /// Where the governor was when a fixed resolution took over, to go back to when Auto returns.
    resume: Option<Rung>,
    pub class: Class,
    rung: Rung,
    /// The governor found, this session, that fewer pixels do not make the game faster here ([`Shape::cpu_bound`]).
    cpu_bound: bool,
    /// Where the device starts: its class's level, at a scale within its budget.
    start: Rung,
    /// What the screen is: device pixels per CSS pixel, and its device pixels in millions.
    dpr: f32,
    pixels: f32,
    scalable: bool,
    governor: Option<Governor>,
    /// Remembered per device, as `build|renderer`: where the governor settled, the next visit's start (a hint: the
    /// governor tries upward from it as soon as there is room).
    signature: String,
    remembered: Option<Rung>,
    /// `?check`: the page may set any field of the detail (`window.hotdogDetail`) and add a cost to each
    /// frame that the graphics can relieve (`window.hotdogBurn`, milliseconds per million pixels drawn).
    check: bool,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    test: String,
    ready: bool,
    /// A quality and a resolution chosen in the game's own menu (menu.rs), taken up by the next frame's `govern`.
    asked: Option<Choice>,
    asked_resolution: Option<Resolution>,
    /// The best Auto may go to: a replay watched on a phone or tablet starts on the device's own start and never goes
    /// above it (a remembered rung above it included). A viewer draws a whole match through other players' eyes, with
    /// nothing of this player's to play, and a phone's browser stops a page outright on memory (iOS Safari: "A problem
    /// repeatedly occurred"), long before it would slow down: the governor still steps down, never up past this. Set
    /// once the device is known ([`decide`]); a level the player picks is theirs.
    pub cap: Option<Rung>,
}

impl Default for Graphics {
    fn default() -> Self {
        let start = Rung { level: Level::High, scale: 1. };
        Graphics {
            choice: Choice::Auto,
            forced: None,
            resolution: Resolution::Auto,
            forced_resolution: None,
            resume: None,
            class: Class::Unknown,
            rung: start,
            cpu_bound: false,
            start,
            dpr: 1.,
            pixels: 0.,
            scalable: false,
            governor: None,
            signature: String::new(),
            remembered: None,
            check: false,
            test: String::new(),
            ready: false,
            asked: None,
            asked_resolution: None,
            cap: None,
        }
    }
}

impl Graphics {
    /// The game's menu (menu.rs) chooses a quality, as the page's menu or F8 does: applied by the next frame, and
    /// remembered (natively in prefs.rs, `graphics`; in a browser where the page keeps it).
    pub fn ask(&mut self, choice: Choice) {
        self.asked = Some(choice);
    }
    /// The game's menu chooses a resolution (a browser only: natively there is no scale to fix).
    pub fn ask_resolution(&mut self, resolution: Resolution) {
        self.asked_resolution = Some(resolution);
    }
    /// Whether the render scale can be chosen (in a browser).
    pub fn scalable(&self) -> bool {
        self.scalable
    }
    fn detail(&self) -> Detail {
        let rung = self.rung;
        let lite = self.forced.is_some_and(|f| f.lite);
        let ceiling = if lite { Quality::Lite } else { best() };
        Detail { scale: rung.scale, ..Detail::of(rung.level, ceiling) }
    }

    /// Where the quality choice starts: for Auto, the remembered rung or the device's start; for a
    /// level, that level at its finest scale within its cap.
    fn anchor(&self) -> Rung {
        match self.choice {
            Choice::Auto => within(self.remembered.unwrap_or(self.start), self.cap),
            Choice::Fixed(level) => picked(level, self.dpr, self.scalable, self.pixels),
        }
    }

    /// The scale the player fixed, where there is a scale to fix (not natively).
    fn fixed_scale(&self) -> Option<f32> {
        match self.resolution {
            Resolution::Fixed(scale) if self.scalable => Some(scale),
            _ => None,
        }
    }

    /// Moves to what the choice says: for Auto, the remembered rung or the device's start, with the
    /// governor; for a level, that level at its finest scale within its cap (or at the fixed
    /// resolution).
    fn follow_choice(&mut self, now: f64) {
        self.resume = None;
        let anchor = self.anchor();
        self.lay(anchor, now);
    }

    /// A resolution chosen. The level stays as it is. A fixed one replaces the scale, and under Auto
    /// quality the governor moves between levels only. Back at Auto, a level picked by hand returns to
    /// its own scale, and under Auto quality the governor to the scale it had when the fixed one took
    /// over (the nearest this level has, if the level has changed since).
    fn choose_resolution(&mut self, resolution: Resolution, now: f64) {
        if self.fixed_scale().is_none() && self.choice == Choice::Auto {
            self.resume = Some(self.rung);
        }
        self.resolution = resolution;
        let level = self.rung.level;
        let anchor = match self.choice {
            Choice::Fixed(_) => self.anchor(),
            Choice::Auto => {
                let resume = self.resume.unwrap_or_else(|| self.anchor());
                if resume.level == level { resume } else { nearest(level, resume.scale, self.dpr, self.scalable, true) }
            }
        };
        if self.fixed_scale().is_none() {
            self.resume = None;
        }
        self.lay(anchor, now);
    }

    /// What the governor moves over: this screen, a fixed resolution if one is chosen, and what it has found.
    fn shape(&self) -> Shape {
        Shape { dpr: self.dpr, scalable: self.scalable, fixed: self.fixed_scale(), cpu_bound: self.cpu_bound }
    }

    /// Puts the game on `anchor` (at the fixed resolution, if one is chosen) and, under Auto, the governor there.
    fn lay(&mut self, anchor: Rung, now: f64) {
        self.rung = match self.fixed_scale() {
            Some(scale) => Rung { level: anchor.level, scale },
            None => anchor,
        };
        let shape = self.shape();
        let cap = self.cap;
        self.governor = (self.choice == Choice::Auto).then(|| Governor { cap, ..Governor::new(self.rung, shape, now) });
    }

    /// One frame of `ms` milliseconds, `busy` of them the main thread's work (None where not measured), for the
    /// governor: the rungs it moved from and to, if it moved.
    fn step(&mut self, now: f64, ms: f32, busy: Option<f32>) -> Option<(Rung, Rung)> {
        let governor = self.governor.as_mut()?;
        let rung = governor.frame(now, ms, busy);
        self.cpu_bound |= governor.shape.cpu_bound;
        let rung = rung?;
        let from = self.rung;
        self.rung = rung;
        Some((from, rung))
    }

    /// Whether where the governor settles is worth remembering for the next visit: not while a fixed
    /// resolution holds it to one scale, which is not where Auto would settle.
    fn learning(&self) -> bool {
        // (Nor under a cap: the viewer's rung says nothing of where the game itself would settle.)
        self.fixed_scale().is_none() && self.cap.is_none()
    }
}

pub struct Plugin;

/// Where natively the quality chosen is kept (prefs.rs).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub const GRAPHICS_KEY: &str = "graphics";

/// The systems that settle the detail each frame; `visuals` runs after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Decide;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Graphics>()
            .init_resource::<work::Work>()
            .add_systems(First, work::start)
            .add_systems(Update, (decide, govern, scale_window).chain().in_set(Decide));
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(bevy::render::Render, work::end.in_set(bevy::render::RenderSystems::PostCleanup));
        }
    }
}

/// What the page knows, natively nothing: `?quality=` and `?scale=`, the stored choices, and the
/// device's facts.
#[derive(Default)]
struct Page {
    forced: String,
    choice: String,
    forced_scale: String,
    scale: String,
    check: bool,
    gpu: String,
    memory: f32,
    cores: u32,
    mobile: bool,
    dpr: f32,
    /// The canvas's size in CSS pixels times the device pixel ratio, in millions of device pixels.
    pixels: f32,
    build: String,
}

fn read_page() -> Page {
    #[cfg(target_arch = "wasm32")]
    {
        let v: serde_json::Value = serde_json::from_str(&web::graphics_page()).unwrap_or_default();
        let text = |key: &str| v[key].as_str().unwrap_or("").to_string();
        Page {
            forced: text("forced"),
            choice: text("choice"),
            forced_scale: text("forcedScale"),
            scale: text("scale"),
            check: v["check"].as_bool().unwrap_or(false),
            gpu: text("gpu"),
            memory: v["memory"].as_f64().unwrap_or(0.) as f32,
            cores: v["cores"].as_u64().unwrap_or(0) as u32,
            mobile: v["mobile"].as_bool().unwrap_or(false),
            dpr: v["dpr"].as_f64().unwrap_or(1.) as f32,
            pixels: v["pixels"].as_f64().unwrap_or(0.) as f32,
            build: text("build"),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        let forced = args.iter().position(|s| s == "--quality").and_then(|i| args.get(i + 1)).cloned().unwrap_or_default();
        // `--quality ultra` and `--ultra` hold High: the Ultra tier is drawn on top of it (ultra.rs).
        let forced = crate::ultra::forced_quality(forced);
        Page { forced, dpr: 1., build: "native".into(), ..default() }
    }
}

/// Decides the start once the renderer is known.
fn decide(
    adapter: Option<Res<RenderAdapterInfo>>,
    time: Res<Time<Real>>,
    options: Res<crate::runtime::Options>,
    mut graphics: ResMut<Graphics>,
    mut commands: Commands,
    mut quality: ResMut<Quality>,
    prefs: Option<Res<crate::prefs::Prefs>>,
) {
    if graphics.ready {
        return;
    }
    let Some(adapter) = adapter else { return };
    #[allow(unused_mut)]
    let mut page = read_page();
    let _ = &prefs;
    // Natively the quality chosen last time (the game's menu, F8) is kept in prefs.rs; a browser's page keeps its own.
    #[cfg(not(target_arch = "wasm32"))]
    if page.choice.is_empty() {
        page.choice = prefs.as_ref().and_then(|p| p.get(GRAPHICS_KEY)).unwrap_or_default();
    }
    // A headset's phone chip draws every picture twice: it starts on the lowest level, until the menu says otherwise.
    if cfg!(target_os = "android") && page.choice.is_empty() {
        page.choice = Level::Lowest.key().into();
    }
    let kind = format!("{:?}", adapter.device_type);
    let native = !cfg!(target_arch = "wasm32");
    let device = Device {
        gpu: format!("{} {}", page.gpu, adapter.name).trim().to_ascii_lowercase(),
        software: kind == "Cpu",
        integrated: native && kind == "IntegratedGpu",
        discrete: native && kind == "DiscreteGpu",
        memory: page.memory,
        cores: page.cores,
        mobile: page.mobile,
    };
    let class = classify(&device);
    let (level, budget) = start(class, &device);
    graphics.dpr = page.dpr.max(0.5);
    graphics.pixels = page.pixels;
    graphics.scalable = cfg!(target_arch = "wasm32") && page.pixels > 0.;
    graphics.class = class;
    graphics.start = first(level, graphics.dpr, graphics.scalable, page.pixels, budget);
    graphics.check = page.check;
    graphics.signature = format!("{}|{}", page.build, device.gpu);
    graphics.forced = Forced::parse(&page.forced);
    graphics.choice = graphics.forced.map(|f| f.choice).or_else(|| Choice::parse(&page.choice)).unwrap_or(Choice::Auto);
    graphics.forced_resolution = Resolution::parse(&page.forced_scale);
    graphics.resolution = graphics.forced_resolution.or_else(|| Resolution::parse(&page.scale)).unwrap_or(Resolution::Auto);
    if let Some((rung, floored)) = recall(&graphics.signature) {
        // Snapped to a scale this screen has for that level. Under the floor only where a governor that keeps
        // to it settled (`floored`): a scale an older one settled on under it starts on the floor again. Only a
        // start: the governor tries upward from it as soon as there is room. (A level an older governor gave up on,
        // `ceiling` in the entry, is no longer kept from it.)
        let (dpr, scalable) = (graphics.dpr, graphics.scalable);
        graphics.remembered = rung.map(|rung| nearest(rung.level, rung.scale, dpr, scalable, floored));
    }
    graphics.cap = viewer_cap(class, !options.replay.is_empty(), graphics.start);
    let now = time.elapsed_secs_f64();
    graphics.follow_choice(now);
    // A profiling run measures one fixed level.
    if options.profile {
        graphics.governor = None;
    }
    graphics.ready = true;
    let detail = graphics.detail();
    *quality = detail.tier;
    info!(
        "Graphics: {} ({}); {} at {}x; {:?}{}, resolution {:?}{}; starts {} at {:.0}% (this device's own start: {} at {:.0}%{})",
        class.name(),
        device.gpu,
        if page.pixels > 0. { format!("{:.1} million device pixels", page.pixels) } else { "native window".into() },
        page.dpr,
        graphics.choice,
        if graphics.forced.is_some() { " set by ?quality" } else { "" },
        graphics.resolution,
        if graphics.forced_resolution.is_some() { " set by ?scale" } else { "" },
        detail.level.name(),
        detail.scale * 100.,
        graphics.start.level.name(),
        graphics.start.scale * 100.,
        graphics.remembered.map(|r| format!("; remembered {} at {:.0}%", r.level.name(), r.scale * 100.)).unwrap_or_default()
            + &graphics.cap.map(|c| format!("; a replay on a phone: no better than {} at {:.0}%", c.level.name(), c.scale * 100.)).unwrap_or_default(),
    );
    commands.insert_resource(detail);
    report(&graphics, &detail);
}

/// The choice, the governor and the `?check` hooks, every frame.
#[allow(unused_variables, unused_mut)]
fn govern(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut graphics: ResMut<Graphics>,
    detail: Option<ResMut<Detail>>,
    mut quality: ResMut<Quality>,
    windows: Query<&Window>,
    prefs: Option<ResMut<crate::prefs::Prefs>>,
    work: Res<work::Work>,
) {
    let Some(mut detail) = detail else { return };
    if !graphics.ready {
        return;
    }
    let now = time.elapsed_secs_f64();
    let ms = time.delta_secs() * 1000.;
    let before = graphics.rung;
    // The player's choices: the quality by F8 here or on the page, the resolution on the page.
    let mut asked = None;
    let mut asked_resolution = None;
    if keys.just_pressed(KeyCode::F8) && graphics.forced.is_none() {
        asked = Some(graphics.choice.next());
    }
    // Read every frame, before anything is reported back to the page, so that a choice made on the page is never
    // overwritten by a report of the old one; the page's choices are fetched (two strings) only when they differ
    // from what the game last told it.
    #[cfg(target_arch = "wasm32")]
    if web::graphics_dirty() {
        if graphics.forced.is_none() {
            if let Some(choice) = Choice::parse(&web::graphics_poll()) {
                if choice != graphics.choice {
                    asked = Some(choice);
                }
            }
        }
        if graphics.forced_resolution.is_none() {
            if let Some(resolution) = Resolution::parse(&web::graphics_resolution_poll()) {
                if resolution != graphics.resolution {
                    asked_resolution = Some(resolution);
                }
            }
        }
    }
    // The game's own menu (menu.rs), after the page's: what it asks is newer than what the page was last told.
    if let Some(choice) = graphics.asked.take().filter(|_| graphics.forced.is_none()) {
        asked = Some(choice);
    }
    if let Some(resolution) = graphics.asked_resolution.take().filter(|_| graphics.forced_resolution.is_none()) {
        asked_resolution = Some(resolution);
    }
    if let Some(choice) = asked {
        graphics.choice = choice;
        graphics.follow_choice(now);
        info!("Graphics: {:?} chosen", choice);
        #[cfg(target_arch = "wasm32")]
        web::graphics_store("hotdog-graphics", choice.key());
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mut prefs) = prefs {
            prefs.set(GRAPHICS_KEY, choice.key());
        }
    }
    if let Some(resolution) = asked_resolution {
        graphics.choose_resolution(resolution, now);
        let rung = graphics.rung;
        info!("Graphics: resolution {:?} chosen; {} at {:.0}%", resolution, rung.level.name(), rung.scale * 100.);
        #[cfg(target_arch = "wasm32")]
        web::graphics_store("hotdog-scale", &resolution.key());
    }
    let mut changed = before != graphics.rung || asked.is_some() || asked_resolution.is_some();
    let graphics = &mut *graphics;
    let cpu_bound = graphics.cpu_bound;
    if let Some((a, b)) = graphics.step(now, ms, work.0) {
        changed = true;
        info!(
            "Graphics: {} at {:.0}% -> {} at {:.0}% ({:.1} ms a frame, {} of it the main thread's)",
            a.level.name(),
            a.scale * 100.,
            b.level.name(),
            b.scale * 100.,
            ms,
            work.0.map_or("none measured".into(), |w| format!("{w:.1} ms"))
        );
    }
    if graphics.cpu_bound && !cpu_bound {
        info!("Graphics: fewer pixels do not make the game faster here; the governor moves the levels only, at full scale");
    }
    // Remember where it settled smoothly.
    if let Some(governor) = graphics.governor.as_ref().filter(|_| graphics.learning()) {
        let settled = governor.settled(now).then_some(graphics.rung);
        if settled.is_some() && settled != graphics.remembered {
            graphics.remembered = settled;
            remember(&graphics.signature, graphics.remembered);
        }
    }
    let mut wanted = graphics.detail();
    // `?check`: the page may override any part of the detail, and burn time in proportion to the pixels
    // drawn and the level, as a slow GPU would.
    #[cfg(target_arch = "wasm32")]
    if graphics.check {
        let test = web::graphics_test();
        if test != graphics.test {
            graphics.test = test.clone();
            info!("Graphics: test detail {test}");
        }
        if !test.is_empty() {
            if let Ok(patch) = serde_json::from_str::<serde_json::Value>(&test) {
                patch_detail(&mut wanted, &patch);
            }
        }
        let weight = match wanted.level {
            Level::High => 1.,
            Level::Medium => 0.75,
            Level::Low => 0.55,
            Level::Lowest => 0.4,
        };
        // The pixels drawn this frame. The time burnt stands in for a GPU's, so it is not the main thread's work.
        let drawn = windows.iter().next().map_or(0., |w| w.physical_width() as f32 * w.physical_height() as f32 / 1e6);
        work::burnt(web::graphics_burn(drawn * weight));
    }
    if *detail != wanted {
        *detail = wanted;
        changed = true;
        if *quality != wanted.tier {
            *quality = wanted.tier;
        }
    }
    if changed {
        report(graphics, &detail);
    }
}

/// Applies `?check`'s `window.hotdogDetail` to a detail.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn patch_detail(detail: &mut Detail, patch: &serde_json::Value) {
    let flag = |key: &str, value: &mut bool| {
        if let Some(b) = patch[key].as_bool() {
            *value = b;
        }
    };
    let number = |key: &str, value: &mut f32| {
        if let Some(n) = patch[key].as_f64() {
            *value = n as f32;
        }
    };
    number("scale", &mut detail.scale);
    number("rays", &mut detail.rays);
    number("plants", &mut detail.plants);
    flag("soft_shadows", &mut detail.soft_shadows);
    flag("lamp_shadows", &mut detail.lamp_shadows);
    flag("msaa", &mut detail.msaa);
    flag("hdr", &mut detail.hdr);
    flag("fog", &mut detail.fog);
    flag("reflections", &mut detail.reflections);
    flag("overlay", &mut detail.overlay);
    flag("lean", &mut detail.lean);
    flag("sharpen", &mut detail.sharpen);
    flag("fxaa", &mut detail.fxaa);
    flag("room_lamps", &mut detail.room_lamps);
    if let Some(n) = patch["shadow_map"].as_u64() {
        detail.shadow_map = n as u32;
    }
    if let Some(n) = patch["lamps"].as_u64() {
        detail.lamps = n as usize;
    }
    match patch["tier"].as_str() {
        Some("full") => detail.tier = Quality::Full,
        Some("lite") => detail.tier = Quality::Lite,
        Some("minimal") => detail.tier = Quality::Minimal,
        _ => {}
    }
    detail.scale = detail.scale.clamp(0.1, 1.);
}

/// In a browser, draws the canvas at the detail's scale and keeps the window's logical size that of
/// the page, so the head-up display and the pointer do not move. Asks the page (a layout read) only when the
/// scale or the canvas has changed.
#[allow(unused_variables, unused_mut)]
fn scale_window(detail: Option<Res<Detail>>, mut windows: Query<&mut Window>, mut seen: Local<Option<(f32, u32, u32)>>) {
    let Some(detail) = detail else { return };
    #[cfg(target_arch = "wasm32")]
    {
        let now = windows.iter().next().map(|w| (detail.scale, w.resolution.physical_width(), w.resolution.physical_height()));
        if now.is_none() || *seen == now {
            return;
        }
        *seen = now;
        let css = web::graphics_scale(detail.scale);
        for mut window in &mut windows {
            let wanted = if detail.scale >= 0.999 || css <= 0. {
                None
            } else {
                Some(window.resolution.physical_width() as f32 / css)
            };
            let now = window.resolution.scale_factor_override();
            let differs = match (now, wanted) {
                (Some(a), Some(b)) => (a - b).abs() > 0.001 * b,
                (None, None) => false,
                _ => true,
            };
            if differs {
                window.resolution.set_scale_factor_override(wanted);
            }
        }
    }
}

/// Tells the page what is shown: the frame-rate line, and the choices its menus mark.
#[allow(unused_variables)]
fn report(graphics: &Graphics, detail: &Detail) {
    #[cfg(target_arch = "wasm32")]
    web::graphics_show(
        &detail.label(graphics.choice, graphics.forced.is_some()),
        graphics.choice.key(),
        graphics.forced.is_some(),
        &graphics.resolution.key(),
        graphics.forced_resolution.is_some(),
    );
}

/// What is remembered for this device: where the governor settled, and whether the governor that settled kept to the
/// floor ([`FLOOR`]; `floor` in the entry).
#[allow(unused_variables)]
fn recall(signature: &str) -> Option<(Option<Rung>, bool)> {
    #[cfg(target_arch = "wasm32")]
    {
        let stored: serde_json::Value = serde_json::from_str(&web::graphics_load("hotdog-graphics-auto")).unwrap_or_default();
        let entry = stored.get(signature)?;
        let rung = entry["level"].as_str().and_then(Level::parse).zip(entry["scale"].as_f64()).map(|(level, scale)| Rung { level, scale: scale as f32 });
        return Some((rung, entry["floor"].as_bool().unwrap_or(false)));
    }
    #[allow(unreachable_code)]
    None
}

#[allow(unused_variables)]
fn remember(signature: &str, rung: Option<Rung>) {
    #[cfg(target_arch = "wasm32")]
    {
        let mut stored: serde_json::Value = serde_json::from_str(&web::graphics_load("hotdog-graphics-auto")).unwrap_or_default();
        if !stored.is_object() {
            stored = serde_json::json!({});
        }
        let mut entry = serde_json::json!({"floor": true});
        if let Some(rung) = rung {
            entry["level"] = rung.level.key().into();
            entry["scale"] = rung.scale.into();
        }
        stored[signature] = entry;
        web::graphics_store("hotdog-graphics-auto", &stored.to_string());
    }
}

/// The main thread's work in each frame, in a browser: from the start of the frame (`First`) to the end of its rendering
/// (the render world's last systems), less what `?check`'s burn spent standing in for a slow GPU. The governor reads it
/// to tell a game held back by its processor from one held back by its pixels, and to see room a display's 60 Hz hides.
/// Natively None: there the render can run on a thread of its own, and presenting waits for the display.
pub mod work {
    use bevy::{platform::time::Instant, prelude::*};
    use std::sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    };

    static EPOCH: OnceLock<Instant> = OnceLock::new();
    static START: AtomicU64 = AtomicU64::new(0);
    static END: AtomicU64 = AtomicU64::new(0);
    static BURNT: AtomicU64 = AtomicU64::new(0);

    fn now() -> f64 {
        EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.
    }
    fn load(cell: &AtomicU64) -> f64 {
        f64::from_bits(cell.load(Ordering::Relaxed))
    }
    fn store(cell: &AtomicU64, value: f64) {
        cell.store(value.to_bits(), Ordering::Relaxed);
    }

    /// The last whole frame's work on the main thread, in milliseconds.
    #[derive(Resource, Default, Clone, Copy, Debug)]
    pub struct Work(pub Option<f32>);

    /// The start of a frame: the last one's work is known now.
    pub fn start(mut work: ResMut<Work>) {
        let (start, end, burnt) = (load(&START), load(&END), load(&BURNT));
        work.0 = (cfg!(target_arch = "wasm32") && start > 0. && end > start).then(|| (end - start - burnt).max(0.) as f32);
        store(&START, now());
        store(&BURNT, 0.);
    }

    /// The end of a frame's rendering.
    pub fn end() {
        store(&END, now());
    }

    /// Milliseconds of a frame spent standing in for a GPU (`?check`'s burn).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn burnt(ms: f64) {
        store(&BURNT, load(&BURNT) + ms);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function graphics_page(){
 const q=new URLSearchParams(location.search),d=window.hotdogDevice||{},c=document.querySelector('#game'),r=c?.parentElement?.getBoundingClientRect();
 let choice='',scale='';try{choice=localStorage.getItem('hotdog-graphics')||'';scale=localStorage.getItem('hotdog-scale')||''}catch{}
 const dpr=window.devicePixelRatio||1;
 return JSON.stringify({forced:q.get('quality')||'',choice:window.hotdogGraphicsChoice||choice,forcedScale:q.get('scale')||'',scale:window.hotdogScaleChoice||scale,check:q.has('check'),gpu:[d.renderer,d.vendor,d.gpuVendor,d.gpuArchitecture,d.gpuDescription].filter(Boolean).join(' '),
  memory:d.memory||navigator.deviceMemory||0,cores:d.cores||navigator.hardwareConcurrency||0,mobile:!!d.mobile,dpr,pixels:r?r.width*r.height*dpr*dpr/1e6:0,build:document.documentElement.dataset.renderer||''});
}
export function graphics_poll(){return String(window.hotdogGraphicsChoice||'')}
export function graphics_dirty(){const s=window.__hotdogShown;return !s||String(window.hotdogGraphicsChoice||'')!==s[0]||String(window.hotdogScaleChoice||'')!==s[1]}
export function graphics_resolution_poll(){return String(window.hotdogScaleChoice||'')}
export function graphics_test(){const t=window.hotdogDetail;return t&&typeof t==='object'?JSON.stringify(t):''}
export function graphics_burn(mpx){const a=Number(window.hotdogBurn)||0;if(a<=0)return 0;const start=performance.now(),end=start+a*mpx;while(performance.now()<end){}return performance.now()-start}
export function graphics_scale(s){
 const root=document.documentElement,v=String(Math.round(s*1000)/1000);
 if(root.style.getPropertyValue('--render-scale')!==v)root.style.setProperty('--render-scale',v);
 const c=document.querySelector('#game'),p=c?.parentElement;return p?p.getBoundingClientRect().width:0;
}
export function graphics_show(text,choice,forced,scale,scaleForced){
 const f=document.querySelector('#fps');if(f)f.dataset.quality=text;
 window.hotdogGraphicsChoice=choice;window.hotdogScaleChoice=scale;window.hotdogGraphics=text;window.__hotdogShown=[choice,scale];
 window.dispatchEvent(new CustomEvent('hotdog:graphics',{detail:{text,choice,forced,scale,scaleForced}}));
}
export function graphics_load(key){try{return localStorage.getItem(key)||''}catch{return ''}}
export function graphics_store(key,value){try{localStorage.setItem(key,value)}catch{}}
")]
    extern "C" {
        pub fn graphics_page() -> String;
        pub fn graphics_poll() -> String;
        pub fn graphics_dirty() -> bool;
        pub fn graphics_resolution_poll() -> String;
        pub fn graphics_test() -> String;
        pub fn graphics_burn(mpx: f32) -> f64;
        pub fn graphics_scale(scale: f32) -> f32;
        pub fn graphics_show(text: &str, choice: &str, forced: bool, scale: &str, scale_forced: bool);
        pub fn graphics_load(key: &str) -> String;
        pub fn graphics_store(key: &str, value: &str);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(gpu: &str) -> Device {
        Device { gpu: gpu.to_ascii_lowercase(), ..default() }
    }

    #[test]
    fn the_tier_table_reads_gpu_names() {
        let cases = [
            ("ANGLE (Intel, Intel(R) UHD Graphics 620 (0x00005917) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::Integrated),
            ("ANGLE (Intel, Intel(R) Iris(R) Xe Graphics (0x00009A49) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::Integrated),
            ("ANGLE (Intel, Mesa Intel(R) Xe Graphics (TGL GT2), OpenGL 4.6)", Class::Integrated),
            ("Intel(R) HD Graphics 400", Class::Integrated),
            ("ANGLE (Intel, Intel(R) Arc(TM) Graphics (0x00007D55) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::StrongIntegrated),
            ("ANGLE (Intel, Vulkan 1.4.354 (Intel(R) Arc(tm) B580 Graphics (BMG G21) (0x0000E20B)), Intel open-source Mesa driver)", Class::Discrete),
            ("ANGLE (Intel, Intel(R) Arc(TM) A770 Graphics (0x000056A0) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::Discrete),
            ("ANGLE (Intel, Intel(R) Arc(TM) A370M Graphics Direct3D11)", Class::Discrete),
            ("ANGLE (AMD, AMD Radeon(TM) Graphics (0x00001638) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::Integrated),
            ("ANGLE (AMD, AMD Radeon Graphics (radeonsi, renoir, LLVM 15.0.7, DRM 3.49), OpenGL 4.6)", Class::Integrated),
            ("ANGLE (AMD, AMD Radeon Vega 8 Graphics Direct3D11 vs_5_0 ps_5_0)", Class::Integrated),
            ("ANGLE (AMD, AMD Radeon 780M Graphics (0x000015BF) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::StrongIntegrated),
            ("ANGLE (AMD, AMD Radeon Graphics (radeonsi, phoenix, LLVM 17), OpenGL 4.6)", Class::StrongIntegrated),
            ("ANGLE (AMD, AMD Radeon RX 6600 XT (0x000073FF) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::Discrete),
            ("ANGLE (AMD, AMD Radeon RX6800 Direct3D11)", Class::Discrete),
            ("ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 (0x00002503) Direct3D11 vs_5_0 ps_5_0, D3D11)", Class::Discrete),
            ("ANGLE (NVIDIA, NVIDIA GeForce GTX 1050 Ti Direct3D11)", Class::Discrete),
            ("ANGLE (NVIDIA, NVIDIA GeForce MX450 Direct3D11 vs_5_0 ps_5_0)", Class::EntryDiscrete),
            ("ANGLE (Apple, ANGLE Metal Renderer: Apple M1, Unspecified Version)", Class::Apple),
            ("Apple GPU", Class::Apple),
            // WebGPU's adapter info alone: vendor and architecture.
            ("intel xe-2hpg", Class::Discrete),
            ("intel xe-lpg", Class::StrongIntegrated),
            ("intel gen-12lp", Class::Integrated),
            ("intel gen-9", Class::Integrated),
            ("nvidia ampere", Class::Discrete),
            ("apple metal-3", Class::Apple),
            ("ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)", Class::Software),
            ("llvmpipe (LLVM 15.0.7, 256 bits)", Class::Software),
            ("ANGLE (Microsoft, Microsoft Basic Render Driver Direct3D11)", Class::Software),
            ("ANGLE (Qualcomm, Adreno (TM) 640, OpenGL ES 3.2)", Class::Phone),
            ("ANGLE (ARM, Mali-G78, OpenGL ES 3.2)", Class::Phone),
            ("ANGLE (Qualcomm, Qualcomm(R) Adreno(TM) X1-85 GPU Direct3D11)", Class::StrongIntegrated),
            ("WebKit WebGL", Class::Unknown),
            ("", Class::Unknown),
        ];
        for (name, class) in cases {
            assert_eq!(classify(&device(name)), class, "{name}");
        }
        // What wgpu says natively counts when the name says nothing; a CPU renderer always.
        assert_eq!(classify(&Device { integrated: true, ..device("some gpu") }), Class::Integrated);
        assert_eq!(classify(&Device { discrete: true, ..device("some gpu") }), Class::Discrete);
        assert_eq!(classify(&Device { software: true, ..device("nvidia geforce rtx 4090") }), Class::Software);
        assert_eq!(classify(&Device { mobile: true, ..device("apple gpu") }), Class::Phone);
    }

    #[test]
    fn each_class_starts_on_its_level() {
        let plain = Device::default();
        assert_eq!(start(Class::Discrete, &plain), (Level::High, None));
        assert_eq!(start(Class::Apple, &plain).0, Level::High);
        assert_eq!(start(Class::StrongIntegrated, &plain).0, Level::Medium);
        assert_eq!(start(Class::EntryDiscrete, &plain).0, Level::Medium);
        assert_eq!(start(Class::Integrated, &plain).0, Level::Low);
        assert_eq!(start(Class::Phone, &plain).0, Level::Low);
        assert_eq!(start(Class::Software, &plain).0, Level::Lowest);
        // Unknown is careful: Medium, or Low on a small machine.
        assert_eq!(start(Class::Unknown, &plain).0, Level::Medium);
        assert_eq!(start(Class::Unknown, &Device { memory: 4., ..default() }).0, Level::Low);
        assert_eq!(start(Class::Unknown, &Device { cores: 4, ..default() }).0, Level::Low);
        assert_eq!(start(Class::Discrete, &Device { memory: 2., ..default() }).0, Level::Low);
    }

    #[test]
    fn the_ladder_runs_through_every_level_never_finer_on_the_way_down() {
        for dpr in [1., 1.25, 1.5, 2., 3.] {
            let shape = Shape::new(dpr, true);
            for anchor in [
                Rung { level: Level::High, scale: 1. },
                first(Level::Medium, dpr, true, 2.07 * dpr * dpr, Some(2.1)),
                first(Level::Low, dpr, true, 2.07 * dpr * dpr, Some(1.2)),
                first(Level::Low, dpr, true, 1.02 * dpr * dpr, Some(1.2)),
                first(Level::Lowest, dpr, true, 2.07 * dpr * dpr, Some(0.6)),
            ] {
                let (rungs, at) = ladder(anchor, shape);
                assert_eq!(rungs[at], anchor);
                assert_eq!(rungs[0].level, Level::High, "{anchor:?} at {dpr}x");
                assert_eq!(rungs.last().unwrap().level, Level::Lowest);
                for level in Level::ALL {
                    assert!(rungs.iter().any(|r| r.level == level), "{level:?} at {dpr}x");
                }
                for pair in rungs.windows(2) {
                    assert!(pair[1].level >= pair[0].level && pair[1].scale <= pair[0].scale, "{pair:?} at {dpr}x, {rungs:?}");
                    assert!(pair[1] != pair[0]);
                }
                // The coarsest is still half the page's own resolution, or near it.
                let last = rungs.last().unwrap();
                assert!(last.scale * dpr >= 0.45, "{last:?} at {dpr}x");
                // Above the last resort every rung keeps to the floor; the last resort is Lowest alone.
                let floor = rungs.iter().position(|r| shape.under(*r)).unwrap_or(rungs.len());
                assert!(rungs[..floor].iter().all(|r| r.scale * dpr >= FLOOR - 0.01), "{rungs:?} at {dpr}x");
                assert!(rungs[floor..].iter().all(|r| r.level == Level::Lowest && r.scale * dpr < FLOOR), "{rungs:?} at {dpr}x");
            }
        }
        // On a 1x screen the features go before the sharpness: the ladder from High.
        let shape = Shape::new(1., true);
        let named = |rungs: &[Rung]| rungs.iter().map(|r| (r.level, (r.scale * 100.).round() as u32)).collect::<Vec<_>>();
        let (rungs, _) = ladder(Rung { level: Level::High, scale: 1. }, shape);
        use Level::*;
        assert_eq!(named(&rungs), [(High, 100), (High, 90), (Medium, 90), (Low, 90), (Low, 80), (Lowest, 80), (Lowest, 70), (Lowest, 62), (Lowest, 55), (Lowest, 50)]);
        assert_eq!(rungs.iter().position(|r| shape.under(*r)), Some(7));
        // And on the way up the sharpness comes back before the features: from Lowest's last resort through its own
        // range to the whole screen, and only then the better levels.
        let (rungs, place) = ladder(Rung { level: Lowest, scale: 0.5 }, shape);
        assert_eq!(named(&rungs[..=place]), [(High, 100), (Medium, 100), (Low, 100), (Lowest, 100), (Lowest, 90), (Lowest, 80), (Lowest, 70), (Lowest, 62), (Lowest, 55), (Lowest, 50)]);
        // From where an integrated GPU starts on a 1080p screen (Low at 70%): Lowest at the same 70% down, Low's own
        // 80% and 90% up.
        let start = first(Low, 1., true, 2.07, Some(1.2));
        assert_eq!(down(start, shape), Some(Rung { level: Lowest, scale: 0.7 }));
        assert_eq!(up(start, shape), Some(Rung { level: Low, scale: 0.8 }));
        // On a 2x screen High comes down to the page's resolution before anything else goes.
        let (rungs, _) = ladder(Rung { level: High, scale: 1. }, Shape::new(2., true));
        assert!(rungs.iter().any(|r| r.level == High && (r.scale - 0.5).abs() < 0.01));
        // Natively: one rung a level.
        assert_eq!(ladder(Rung { level: Low, scale: 1. }, Shape::new(1., false)), (Level::ALL.map(|level| Rung { level, scale: 1. }).to_vec(), 2));
        // With the processor as the limit: the levels alone, each step up to a level's whole resolution first, and
        // only the levels that cost the processor less than the one above.
        let flat = Shape { cpu_bound: true, ..shape };
        let (rungs, place) = ladder(Rung { level: Lowest, scale: 0.7 }, flat);
        assert_eq!(rungs[place - 1], Rung { level: Lowest, scale: 1. });
        assert!(rungs[..place].iter().all(|r| r.scale == 1.), "{rungs:?}");
        assert!(!rungs.iter().any(|r| flat.under(*r)));
        let levels: Vec<Level> = rungs[..place].iter().map(|r| r.level).collect();
        assert_eq!(levels[0], High);
        for pair in levels.windows(2) {
            assert!(cpu(pair[1]) <= cpu(pair[0]) * SAVES, "{levels:?}");
        }
        // A fixed resolution: the levels alone, at that scale.
        let fixed = Shape { fixed: Some(0.9), ..shape };
        assert_eq!(ladder(Rung { level: Medium, scale: 0.9 }, fixed), (Level::ALL.map(|level| Rung { level, scale: 0.9 }).to_vec(), 1));
    }

    #[test]
    fn a_start_fits_its_budget() {
        // A 1080p laptop (2.07 million pixels) with integrated graphics starts Low within 1.2 million.
        let rung = first(Level::Low, 1., true, 2.07, Some(1.2));
        assert_eq!(rung.level, Level::Low);
        assert!(2.07 * rung.scale * rung.scale <= 1.21 && rung.scale >= 0.7, "{rung:?}");
        // A 1280x800 one draws every pixel.
        assert_eq!(first(Level::Low, 1., true, 1.02, Some(1.2)).scale, 1.);
        // A discrete GPU: full scale whatever the screen.
        assert_eq!(first(Level::High, 2., true, 8.3, None).scale, 1.);
        // A level the player picks keeps within its cap: Low on a 4K screen at 2x draws no more than 1080p.
        let rung = picked(Level::Low, 2., true, 8.3);
        assert!(8.3 * rung.scale * rung.scale <= 2.11, "{rung:?}");
        // But never under the floor: at 1x that screen's Low draws 70% each way.
        assert_eq!(picked(Level::Low, 1., true, 8.3).scale, 0.7);
        assert_eq!(first(Level::Lowest, 1., true, 8.3, Some(0.6)).scale, 0.7);
        assert_eq!(picked(Level::High, 2., true, 8.3).scale, 1.);
        // Natively there is no scale.
        assert_eq!(picked(Level::Lowest, 1., false, 8.3).scale, 1.);
    }

    #[test]
    fn a_fixed_resolution_overrides_the_governors_scale_and_auto_gives_it_back() {
        // A 1080p laptop with integrated graphics: Auto starts on Low at 70%, with the governor.
        let start = first(Level::Low, 1., true, 2.07, Some(1.2));
        let laptop = || Graphics { dpr: 1., pixels: 2.07, scalable: true, start, ..default() };
        let mut graphics = laptop();
        graphics.follow_choice(0.);
        let shown = |g: &Graphics| (g.detail().level, g.detail().scale);
        assert_eq!(shown(&graphics), (Level::Low, 0.7));
        // 90% chosen: that scale at the same level, and the governor still at work.
        graphics.choose_resolution(Resolution::Fixed(0.9), 1.);
        assert_eq!(shown(&graphics), (Level::Low, 0.9));
        assert!(graphics.governor.is_some());
        assert!(!graphics.learning(), "where it settles at a scale Auto did not choose is not remembered");
        // Too slow: the governor moves the level down, never the scale.
        let (mut now, mut moves) = (1., Vec::new());
        while now < 60. {
            now += 0.03;
            moves.extend(graphics.step(now, 30., Some(5.)));
        }
        assert!(!moves.is_empty());
        assert!(moves.iter().all(|(a, b)| a.scale == 0.9 && b.scale == 0.9 && b.level > a.level), "{moves:?}");
        assert_eq!(shown(&graphics), (Level::Lowest, 0.9));
        // Another share: the level stays.
        graphics.choose_resolution(Resolution::Fixed(0.5), now);
        assert_eq!(shown(&graphics), (Level::Lowest, 0.5));
        // Auto again: the governor's 70% back, at the level it has come to, with the scale its own to move again.
        graphics.choose_resolution(Resolution::Auto, now);
        assert_eq!(shown(&graphics), (Level::Lowest, 0.7));
        assert!(graphics.learning() && graphics.governor.is_some());
        assert!(down(graphics.rung, graphics.shape()).is_some_and(|r| r.scale < 0.7), "the scale is the governor's to move again");
        // And straight back, with nothing moved meanwhile: the very rung it was on.
        let mut graphics = laptop();
        graphics.follow_choice(0.);
        let was = graphics.rung;
        graphics.choose_resolution(Resolution::Fixed(0.6), 1.);
        graphics.choose_resolution(Resolution::Auto, 2.);
        assert_eq!(graphics.rung, was);

        // A level picked by hand: a fixed resolution goes past its cap, and Auto brings back its own scale.
        let mut graphics = Graphics { choice: Choice::Fixed(Level::Low), pixels: 8.3, dpr: 2., ..laptop() };
        graphics.follow_choice(0.);
        assert_eq!(shown(&graphics), (Level::Low, 0.5));
        graphics.choose_resolution(Resolution::Fixed(1.), 1.);
        assert_eq!(shown(&graphics), (Level::Low, 1.));
        assert!(graphics.governor.is_none());
        graphics.choose_resolution(Resolution::Auto, 2.);
        assert_eq!(shown(&graphics), (Level::Low, 0.5));
        // A level chosen while the resolution is fixed keeps the resolution.
        graphics.choose_resolution(Resolution::Fixed(0.8), 3.);
        graphics.choice = Choice::Fixed(Level::High);
        graphics.follow_choice(4.);
        assert_eq!(shown(&graphics), (Level::High, 0.8));

        // Natively there is no scale to fix.
        let mut graphics = Graphics { scalable: false, start: first(Level::Low, 1., false, 2.07, Some(1.2)), ..laptop() };
        graphics.follow_choice(0.);
        graphics.choose_resolution(Resolution::Fixed(0.5), 1.);
        assert_eq!(graphics.detail().scale, 1.);
        assert!(graphics.learning());
    }

    #[test]
    fn resolution_names() {
        for (text, want) in [
            ("auto", Some(Resolution::Auto)),
            (" Auto ", Some(Resolution::Auto)),
            ("80", Some(Resolution::Fixed(0.8))),
            ("80%", Some(Resolution::Fixed(0.8))),
            ("0.8", Some(Resolution::Fixed(0.8))),
            ("100", Some(Resolution::Fixed(1.))),
            ("1", Some(Resolution::Fixed(1.))),
            ("50", Some(Resolution::Fixed(0.5))),
            ("25", Some(Resolution::Fixed(0.25))),
            ("20", None),
            ("1%", None),
            ("120", None),
            ("-50", None),
            ("NaN", None),
            ("high", None),
            ("", None),
        ] {
            assert_eq!(Resolution::parse(text), want, "{text:?}");
        }
        // What the page writes is read back as it was.
        for key in ["auto", "100", "90", "80", "70", "60", "50"] {
            assert_eq!(Resolution::parse(key).unwrap().key(), key);
        }
    }

    #[test]
    fn old_and_new_quality_names() {
        assert_eq!(Forced::parse("minimal"), Some(Forced { choice: Choice::Fixed(Level::Lowest), lite: false }));
        assert_eq!(Forced::parse("lite"), Some(Forced { choice: Choice::Fixed(Level::High), lite: true }));
        assert_eq!(Forced::parse("full"), Some(Forced { choice: Choice::Fixed(Level::High), lite: false }));
        assert_eq!(Forced::parse("Low").map(|f| f.choice), Some(Choice::Fixed(Level::Low)));
        assert_eq!(Forced::parse("auto").map(|f| f.choice), Some(Choice::Auto));
        assert_eq!(Forced::parse("ultra"), None);
        assert_eq!(Detail::of(Level::Lowest, Quality::Full).tier, Quality::Minimal);
        assert_eq!(Detail::of(Level::High, Quality::Lite).tier, Quality::Lite);
        assert_eq!(Detail::of(Level::High, Quality::Full), Detail { level: Level::High, ..Detail::of(Level::High, Quality::Full) });
    }

    /// Feeds the governor frames of `frame(t, rung)` (the frame's milliseconds and the main thread's work in it, None
    /// where not measured) for `seconds`, at the frame rate they imply; returns every move it made, with its time.
    fn run_busy(governor: &mut Governor, from: f64, seconds: f64, frame: impl Fn(f64, Rung) -> (f32, Option<f32>)) -> (f64, Vec<(f64, Rung)>) {
        let mut now = from;
        let mut moves = Vec::new();
        while now < from + seconds {
            let (ms, busy) = frame(now, governor.rung);
            now += ms as f64 / 1000.;
            if let Some(rung) = governor.frame(now, ms, busy) {
                moves.push((now, rung));
            }
        }
        (now, moves)
    }

    /// The same with nothing known of the main thread.
    fn run(governor: &mut Governor, from: f64, seconds: f64, ms: impl Fn(f64, Rung) -> f32) -> (f64, Vec<(f64, Rung)>) {
        run_busy(governor, from, seconds, |t, r| (ms(t, r), None))
    }

    /// A frame that needs `cost` milliseconds, on a 60 Hz display with vsync.
    fn vsync(cost: f32) -> f32 {
        (cost / 16.667).ceil().max(1.) * 16.667
    }

    /// The ladder from High on a 1x screen, and each rung's place on it.
    fn rungs() -> (Vec<Rung>, impl Fn(Rung) -> usize) {
        let (rungs, _) = ladder(Rung { level: Level::High, scale: 1. }, Shape::new(1., true));
        let all = rungs.clone();
        (rungs, move |r: Rung| all.iter().position(|x| same(*x, r)).unwrap_or(usize::MAX))
    }

    /// The tries up in `moves` from `rung`, and when.
    fn tries_from(moves: &[(f64, Rung)], start: Rung, rung: Rung) -> Vec<f64> {
        let mut at = start;
        let mut out = vec![];
        for &(t, to) in moves {
            if same(at, rung) && to != rung && to.level <= rung.level && (to.level < rung.level || to.scale > rung.scale) {
                out.push(t);
            }
            at = to;
        }
        out
    }

    #[test]
    fn the_governor_steps_down_until_the_game_is_smooth_and_stays() {
        let (rungs, place) = rungs();
        // A GPU that holds the game back: each rung a fifth cheaper than the one above, the main thread idle most of the
        // frame; the device manages 60 frames a second from rung 3.
        let cost = |r: Rung| 30. * 0.8f32.powi(place(r) as i32);
        let mut governor = Governor::new(rungs[0], Shape::new(1., true), 0.);
        let (now, moves) = run_busy(&mut governor, 0., 300., |_, r| (vsync(cost(r)), Some(4.)));
        let settled = governor.rung;
        assert!(cost(settled) <= 16.667 && cost(rungs[place(settled) - 1]) > 16.667, "settles on the first smooth rung, {moves:?}");
        assert!(!governor.shape.cpu_bound, "steps that the display's 60 Hz hid were the GPU's: {moves:?}");
        // Nothing during the first seconds; then one step every few seconds, one rung at a time.
        assert!(moves[0].0 >= QUIET_START + SLOW_FOR, "{moves:?}");
        let downs: Vec<&(f64, Rung)> = moves.iter().take(place(settled)).collect();
        assert!(downs.windows(2).all(|w| place(w[1].1) == place(w[0].1) + 1 && w[1].0 - w[0].0 >= QUIET_SCALE + SLOW_FOR), "{moves:?}");
        assert!(downs.last().unwrap().0 < 40., "smooth within 40 seconds: {moves:?}");
        // A 60 Hz display hides the headroom, so it tries the rung above now and then: each time after a longer wait,
        // and each try is over within a few seconds.
        let (later_now, later) = run_busy(&mut governor, now, 900., |_, r| (vsync(cost(r)), Some(4.)));
        let mut all = moves.clone();
        all.extend(later.iter().copied());
        let tries = tries_from(&all, rungs[0], settled);
        assert!(tries.len() >= 2 && tries.len() <= 7, "{tries:?}");
        for (k, pair) in tries.windows(2).enumerate() {
            assert!(pair[1] - pair[0] >= BACKOFF * 2f64.powi(k as i32), "the waits grow: {tries:?}");
        }
        let away: f64 = all.windows(2).filter(|w| !same(w[0].1, settled)).map(|w| w[1].0 - w[0].0).sum();
        assert!(away < 0.05 * 1200., "{away} s off the settled rung");
        assert_eq!(governor.rung, settled);
        assert!(governor.settled(later_now) || later.last().is_some_and(|m| later_now - m.0 < SETTLE));
    }

    #[test]
    fn hitches_do_not_count_as_slowness() {
        // 60 Hz with two 150 ms hitches in the window (a shader compiling, a round starting): its pace is
        // still 60 frames a second.
        let pace_of = |frames: &[f32]| pace(frames.iter().copied());
        let mut frames = vec![16.667; 148];
        frames.extend([150.; 2]);
        assert!(pace_of(&frames).mean < 17.);
        // Hitches every tenth frame are no longer hitches: that is 33 frames a second.
        let mut frames = vec![16.667; 135];
        frames.extend([150.; 15]);
        assert!(pace_of(&frames).mean > TARGET_MS);
        // A frame late now and then (33 ms) is real slowness and counts.
        let mut frames = vec![16.667; 100];
        frames.extend([33.33; 50]);
        assert!(pace_of(&frames).mean > TARGET_MS);
        // A CPU renderer's 150 ms frames are its pace, not hitches.
        assert!(pace_of(&[150.; 20]).mean > 140.);
        // Six quick frames and a long wait for the GPU, over and over: 18 ms a frame, not 5.
        let frames: Vec<f32> = (0..140).map(|i| if i % 7 == 6 { 100. } else { 5. }).collect();
        assert!(pace_of(&frames).mean > TARGET_MS * 0.85, "{:?}", pace_of(&frames));
        // A frame rate that reads 56 to 64 around a 60 Hz cap holds it.
        let frames: Vec<f32> = (0..150).map(|i| 1000. / (56. + (i * 37 % 9) as f32)).collect();
        assert!(pace_of(&frames).mean <= HOLD_MS, "{:?}", pace_of(&frames));
    }

    #[test]
    fn the_governor_ignores_hitches_and_the_tab_being_hidden() {
        let (rungs, place) = rungs();
        let mut governor = Governor::new(rungs[2], Shape::new(1., true), 0.);
        // 8 ms frames with a 250 ms hitch every 4 seconds (a round starting) and a 2 second gap (the
        // tab hidden) now and then: nothing to do.
        let (_, moves) = run(&mut governor, 0., 60., |t, _| {
            if (t % 4.) < 0.01 {
                250.
            } else if (t % 17.) < 0.01 {
                2000.
            } else {
                8.
            }
        });
        // It may well step up (8 ms is plenty of room), never down.
        let ups = |moves: &[(f64, Rung)]| {
            let mut at = rungs[2];
            moves.iter().all(|m| {
                let ok = up(at, Shape::new(1., true)) == Some(m.1);
                at = m.1;
                ok
            })
        };
        assert!(ups(&moves), "{moves:?}");
        let mut governor = Governor::new(rungs[2], Shape::new(1., true), 0.);
        let (_, moves) = run(&mut governor, 0., 60., |t, _| if (t % 4.) < 0.01 { 250. } else { 16.667 });
        assert!(ups(&moves), "a hitch is not slowness: {moves:?}");
        let _ = place;
    }

    #[test]
    fn the_governor_steps_back_up_with_room_to_spare() {
        let (rungs, place) = rungs();
        let last = *rungs.last().unwrap();
        let mut governor = Governor::new(last, Shape::new(1., true), 0.);
        // Uncapped and fast: it climbs all the way, waiting longer before each better level.
        let (now, moves) = run(&mut governor, 0., 400., |_, _| 6.);
        assert_eq!(governor.rung, rungs[0], "{moves:?}");
        let mut at = last;
        for (t, (when, to)) in moves.iter().enumerate() {
            let level = to.level != at.level;
            assert_eq!(up(at, Shape::new(1., true)), Some(*to), "one step at a time: {moves:?}");
            if t > 0 {
                assert!(when - moves[t - 1].0 >= if level { UP_LEVEL_FOR } else { UP_FOR }, "{moves:?}");
            }
            at = *to;
        }
        // Then the GPU gets slower (a bigger window, a hot laptop): it comes down, and tries above now and then.
        let cost = |r: Rung| 40. * 0.82f32.powi(place(r) as i32);
        let (now, moves) = run_busy(&mut governor, now, 400., |_, r| (vsync(cost(r)), Some(3.)));
        let settled = governor.rung;
        assert!(cost(settled) <= 16.667 && cost(rungs[place(settled) - 1]) > 16.667, "{moves:?}");
        let (_, later) = run_busy(&mut governor, now, 300., |_, r| (vsync(cost(r)), Some(3.)));
        assert!(later.len() <= 4, "no oscillation: {later:?}");
        assert_eq!(governor.rung, settled);
    }

    /// A replay watched on a phone keeps to the device's start: a fast game climbs no higher than it, and a slow one
    /// still comes down; elsewhere, and in the game itself, nothing is capped.
    #[test]
    fn a_replay_on_a_phone_never_climbs_above_the_devices_start() {
        let (rungs, _) = rungs();
        let cap = rungs[rungs.len() / 2];
        let last = *rungs.last().unwrap();
        let mut governor = Governor { cap: Some(cap), ..Governor::new(last, Shape::new(1., true), 0.) };
        let (now, moves) = run(&mut governor, 0., 600., |_, _| 6.);
        assert!(!moves.is_empty(), "it climbs to the cap");
        assert!(moves.iter().all(|(_, r)| !above(*r, cap)), "never past the cap: {moves:?}");
        assert!(same(governor.rung, cap) || !above(up(governor.rung, Shape::new(1., true)).unwrap(), cap), "{moves:?}");
        let (_, down) = run(&mut governor, now, 60., |_, _| 40.);
        assert!(down.iter().any(|(_, r)| above(cap, *r)), "and still steps down: {down:?}");

        let low = Rung { level: Level::Low, scale: 0.5 };
        assert_eq!(viewer_cap(Class::Phone, true, low), Some(low));
        assert_eq!(viewer_cap(Class::Phone, false, low), None, "the game itself is not capped");
        assert_eq!(viewer_cap(Class::Apple, true, low), None, "nor a viewer elsewhere");
        // A remembered rung above the cap starts at the cap; one below it stays.
        assert_eq!(within(Rung { level: Level::High, scale: 1. }, Some(low)), low);
        assert_eq!(within(Rung { level: Level::Low, scale: 0.8 }, Some(low)), low);
        let lower = Rung { level: Level::Lowest, scale: 0.7 };
        assert_eq!(within(lower, Some(low)), lower);
        assert_eq!(within(lower, None), lower);
    }

    #[test]
    fn at_60_hz_a_better_level_is_tried_now_and_then_and_each_failure_waits_longer() {
        // Start on Medium at the whole resolution, which runs at 60 Hz; High is too much. Nothing is known of the main
        // thread (natively): High is tried after a long while held at 60 Hz.
        let start = Rung { level: Level::Medium, scale: 1. };
        let cost = |r: Rung| if r.level == Level::High { 25. } else { 12. };
        let mut governor = Governor::new(start, Shape::new(1., true), 0.);
        let (_, moves) = run(&mut governor, 0., 900., |_, r| vsync(cost(r)));
        let tries = tries_from(&moves, start, start);
        assert!(tries[0] >= QUIET_START + UP_LEVEL_CAPPED_FOR, "{moves:?}");
        assert!(tries.len() >= 3 && tries.len() <= 6, "{tries:?}");
        for (k, pair) in tries.windows(2).enumerate() {
            assert!(pair[1] - pair[0] >= BACKOFF * 2f64.powi(k as i32), "{tries:?}");
        }
        assert_eq!(governor.rung, start);
        assert!(governor.failures(Rung { level: Level::High, scale: 1. }) as usize == tries.len());
        // With the main thread measured and too busy for High, it is not tried at all.
        let mut governor = Governor::new(start, Shape::new(1., true), 0.);
        let (_, moves) = run_busy(&mut governor, 0., 600., |_, r| (vsync(cost(r)), Some(12.)));
        assert!(moves.iter().all(|m| m.1.level != Level::High), "{moves:?}");
    }

    /// The ladder from High on a 1x screen, the floor (the last rung before the last resort), and a device that needs
    /// `at_floor` ms a frame there: a quarter more for each rung above, and in proportion to the pixels under it.
    fn floored(at_floor: f32) -> (Vec<Rung>, Rung, impl Fn(Rung) -> f32) {
        let (rungs, place) = rungs();
        let floor = rungs[rungs.iter().position(|r| Shape::new(1., true).under(*r)).unwrap() - 1];
        let scales: Vec<f32> = rungs.iter().map(|r| r.scale).collect();
        let f = place(floor);
        let cost = move |r: Rung| {
            let i = place(r);
            if i <= f { at_floor * 1.25f32.powi((f - i) as i32) } else { at_floor * (scales[i] / scales[f]).powi(2) }
        };
        (rungs, floor, cost)
    }

    #[test]
    fn auto_holds_the_floor_while_lowest_runs_at_24_fps_or_better() {
        // 30 ms at Lowest on the floor (33 frames a second): everything else goes, and the picture keeps its 70%.
        let (rungs, floor, cost) = floored(30.);
        let mut governor = Governor::new(rungs[0], Shape::new(1., true), 0.);
        let (now, moves) = run_busy(&mut governor, 0., 600., |_, r| (cost(r), Some(5.)));
        assert_eq!(governor.rung, floor, "{moves:?}");
        assert_eq!(floor, Rung { level: Level::Lowest, scale: 0.7 });
        assert!(moves.iter().all(|m| !governor.shape.under(m.1)), "never under the floor: {moves:?}");
        assert!(governor.settled(now), "and that is where it settles");
    }

    #[test]
    fn under_24_fps_auto_goes_under_the_floor_slowly_and_comes_back_with_room() {
        // 60 ms at the floor: the last resort, one step at a time, each after a long slow while, to the
        // first rung under 24 frames a second (0.62 is 47 ms, 0.55 is 37 ms).
        let (_, floor, cost) = floored(60.);
        let mut governor = Governor::new(floor, Shape::new(1., true), 0.);
        let (now, moves) = run_busy(&mut governor, 0., 300., |_, r| (cost(r), Some(5.)));
        assert_eq!(governor.rung, Rung { level: Level::Lowest, scale: 0.55 }, "{moves:?}");
        assert_eq!(moves.len(), 2, "{moves:?}");
        assert!(moves[0].0 >= QUIET_START + LAST_RESORT_FOR, "{moves:?}");
        assert!(moves[1].0 - moves[0].0 >= QUIET_SCALE + LAST_RESORT_FOR, "{moves:?}");
        // And it stays there: 37 ms is not room enough to try 0.62 again.
        let (now, later) = run_busy(&mut governor, now, 300., |_, r| (cost(r), Some(5.)));
        assert!(later.is_empty(), "{later:?}");
        assert!(governor.settled(now));
        // The machine gets twice as fast (another program stops): with room it climbs back to the floor, a step
        // at a time, and there (30 ms) it stays.
        let (_, back) = run_busy(&mut governor, now, 300., |_, r| (cost(r) / 2., Some(5.)));
        assert_eq!(back.iter().map(|m| m.1).collect::<Vec<_>>(), [Rung { level: Level::Lowest, scale: 0.62 }, floor], "{back:?}");
        assert!(back[1].0 - back[0].0 >= UP_FOR, "{back:?}");
    }

    #[test]
    fn a_step_of_scale_that_does_not_help_is_taken_back_and_the_resolution_goes_back_to_the_whole_screen() {
        // A game held back by its processor, with nothing known of the main thread: 50 ms a frame whatever is drawn.
        // The last resort is tried, gains nothing, and is taken back at once; from then on the scale is left alone, and
        // goes back up to the whole screen, which is no slower.
        let (_, floor, _) = floored(50.);
        let mut governor = Governor::new(floor, Shape::new(1., true), 0.);
        let (_, moves) = run(&mut governor, 0., 600., |_, _| 50.);
        let path: Vec<Rung> = moves.iter().map(|m| m.1).collect();
        assert_eq!(path, [Rung { level: Level::Lowest, scale: 0.62 }, floor, Rung { level: Level::Lowest, scale: 1. }], "{moves:?}");
        assert!(moves[1].0 - moves[0].0 < QUIET_SCALE + WINDOW + 2. * LOOK, "taken back at the first full look: {moves:?}");
        assert!(governor.shape.cpu_bound);
        // The same higher up, at every step of scale: High at 100% held back by the processor goes to Medium at 100%,
        // never to High at 90%.
        let (rungs, _) = rungs();
        let mut governor = Governor::new(rungs[0], Shape::new(1., true), 0.);
        let (_, moves) = run(&mut governor, 0., 120., |_, r| if r.level == Level::High { 30. } else { 15. });
        assert_eq!(governor.rung, Rung { level: Level::Medium, scale: 1. }, "{moves:?}");
        assert!(moves.iter().all(|m| m.1.scale >= 0.9));
    }

    #[test]
    fn a_game_whose_main_thread_is_full_never_lowers_the_resolution() {
        // The main thread at work the whole of a slow frame: the scale is not touched; the levels that cost the processor
        // less go, at full resolution.
        let (rungs, _) = rungs();
        // (A browser starts the next frame as soon as a late one is done: such frames are not rounded up to the display's.)
        let ms = |r: Rung| 34. * cpu(r.level) / cpu(Level::High);
        let mut governor = Governor::new(rungs[0], Shape::new(1., true), 0.);
        let (_, moves) = run_busy(&mut governor, 0., 300., |_, r| (ms(r).max(16.667) + 1.5, Some(ms(r))));
        assert!(moves.iter().all(|m| m.1.scale == 1.), "{moves:?}");
        assert!(governor.shape.cpu_bound);
        assert!(governor.rung.level > Level::High, "{moves:?}");
        // A GPU that holds the game back (the main thread mostly idle) still gets its fewer pixels.
        let mut governor = Governor::new(rungs[0], Shape::new(1., true), 0.);
        let (_, moves) = run_busy(&mut governor, 0., 300., |_, r| (vsync(26. * r.scale * r.scale), Some(4.)));
        assert!(moves.iter().any(|m| m.1.scale < 1.), "{moves:?}");
        assert!(!governor.shape.cpu_bound);
    }

    /// The user's machine (AMD integrated graphics, a 60 Hz display, WebGL 2): at Lowest it reads 56 to 64 frames a
    /// second at 100% and a steady 60 at 50%; each better level costs its processor more ([`CPU`]) and its GPU more.
    /// Its main thread does `cpu_ms` at Lowest, and its GPU `gpu_ms` for Lowest's whole screen.
    fn users_machine(cpu_ms: f32, gpu_ms: f32) -> impl Fn(f64, Rung) -> (f32, Option<f32>) {
        move |t, r| {
            let gpu = match r.level {
                Level::High => 1.9,
                Level::Medium => 1.5,
                Level::Low => 1.15,
                Level::Lowest => 1.,
            } * gpu_ms
                * r.scale
                * r.scale;
            let busy = cpu_ms * cpu(r.level);
            let cost = busy.max(gpu);
            // At the display's rate, with the jitter a browser's frame clock has (the 56 to 64 it reads); slower, whole
            // refreshes as often as the cost asks.
            let jitter = ((t * 7919.).sin() as f32) * 1.2;
            let ms = if cost <= 16.4 { 16.667 + jitter } else if busy >= gpu { busy + 1.5 } else { vsync(cost) };
            (ms, Some(busy))
        }
    }

    #[test]
    fn the_users_machine_ends_at_its_whole_resolution_and_the_best_level_it_holds() {
        // Lowest: 9 ms of main thread, 14 ms of GPU at 100% (3.5 at 50%): it holds 60 at either. Low then needs 11.7 ms
        // and 16.1 ms (holds); Medium 14.4 and 21 ms (does not).
        let machine = users_machine(9., 14.);
        let shape = Shape::new(1., true);
        for start in [
            // Where an integrated GPU starts on a 1080p screen, where an old governor left it (snapped to the floor),
            // and the very bottom.
            first(Level::Low, 1., true, 2.07, Some(1.2)),
            nearest(Level::Lowest, 0.5, 1., true, false),
            Rung { level: Level::Lowest, scale: 0.5 },
        ] {
            let mut governor = Governor::new(start, shape, 0.);
            let (now, moves) = run_busy(&mut governor, 0., 600., &machine);
            let best = Rung { level: Level::Low, scale: 1. };
            assert_eq!(governor.rung, best, "from {start:?}: {moves:?}");
            let reached = moves.iter().find(|m| same(m.1, best)).unwrap().0;
            assert!(reached < 90., "from {start:?}, at the whole resolution and Low within 90 s: {moves:?}");
            assert!(!governor.shape.cpu_bound);
            // Medium is tried now and then and given up each time, sooner and sooner.
            let tries = tries_from(&moves, start, best);
            assert!(tries.len() <= 6, "{tries:?}");
            // In ten minutes it spent under a twentieth of the time anywhere but there, after reaching it.
            let away: f64 = moves.windows(2).filter(|w| w[0].0 > reached && !same(w[0].1, best)).map(|w| w[1].0 - w[0].0).sum();
            assert!(away < 0.05 * (now - reached), "{away} s away: {moves:?}");
        }
        // A machine whose processor is the limit at Lowest (16 ms of main thread: 56 to 64 at either scale) stays at
        // Lowest, and at its whole resolution; so does one with 11 ms there, too little room for Low's 14.3 ms: neither
        // tries Low at all.
        for cpu_ms in [15.9, 11.] {
            let machine = users_machine(cpu_ms, 6.);
            let mut governor = Governor::new(Rung { level: Level::Lowest, scale: 0.7 }, shape, 0.);
            let (_, moves) = run_busy(&mut governor, 0., 600., &machine);
            assert_eq!(governor.rung, Rung { level: Level::Lowest, scale: 1. }, "{cpu_ms}: {moves:?}");
            assert!(moves.iter().all(|m| m.1.level == Level::Lowest), "{cpu_ms}: {moves:?}");
        }
    }

    #[test]
    fn a_scale_an_older_governor_settled_on_under_the_floor_starts_on_it() {
        // The old governor went to 50% at Lowest for any game under 50 frames a second; this one starts there
        // on the floor and goes under it only if it has to.
        assert_eq!(nearest(Level::Lowest, 0.5, 1., true, false), Rung { level: Level::Lowest, scale: 0.7 });
        // Where this governor settled under the floor is where it starts.
        assert_eq!(nearest(Level::Lowest, 0.55, 1., true, true), Rung { level: Level::Lowest, scale: 0.55 });
        let shape = Shape::new(1., true);
        assert!(shape.under(Rung { level: Level::Lowest, scale: 0.55 }));
        assert!(!shape.under(Rung { level: Level::Lowest, scale: 0.7 }));
        // Natively there is no scale and so no last resort.
        let (rungs, _) = ladder(Rung { level: Level::High, scale: 1. }, Shape::new(1., false));
        assert!(!rungs.iter().any(|r| Shape::new(1., false).under(*r)));
    }

    #[test]
    fn the_governor_forgives_a_slow_patch_and_climbs_back() {
        let (rungs, place) = rungs();
        // Uncapped: 12 ms a frame at full detail, but 30 s of a horde costing 20 ms more.
        let mut governor = Governor::new(rungs[0], Shape::new(1., true), 0.);
        let (_, moves) = run_busy(&mut governor, 0., 240., |t, r| {
            let k = 0.8f32.powi(place(r) as i32);
            let gpu = 12. * k;
            (if (30. ..60.).contains(&t) { gpu + 20. * k } else { gpu }, Some(3.))
        });
        assert!(moves.iter().any(|m| place(m.1) > 0), "it stepped down in the horde: {moves:?}");
        assert_eq!(governor.rung, rungs[0], "and back up after it: {moves:?}");
        assert!(governor.settled(240.));
    }
}

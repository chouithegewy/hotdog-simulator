// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The Ultra tier: a native-only top tier built on Bevy 0.19's two experimental renderers (README, "Ultra tier (native)").
//!
//! * **Raytraced lighting** (`--raytraced`, `gi`): `bevy_solari`'s real-time direct and indirect light, traced with
//!   hardware ray queries against the scene. The sun's light bounces off the counter and the walls, emissive things light
//!   what is round them, and shadows are traced and soft. See `ultra/gi.rs` for what the scene has to do for it.
//! * **Virtual geometry** (`--meshlets`): Bevy's meshlet renderer draws a few things at far more triangles than the normal
//!   budget allows (the counter top, a tiled backsplash, the steamers' woven lids, cereal boxes, plates), converted once
//!   and cached on disk. See `ultra/geometry.rs`.
//! * `--ultra` or `--quality ultra` asks for both, at the High level. In Bevy 0.19.1 the two cannot run together, so both
//!   asked for gets the raytraced lighting, and says why (see [`decide`]).
//!
//! Everything here is compiled only with the `ultra` cargo feature, natively; without it the flags only say how to build it.
//! Nothing changes unless one of the flags is given.
use bevy::prelude::*;

#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
mod geometry;
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
mod gi;
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
mod shapes;

/// What the command line asked of the Ultra tier.
#[derive(Resource, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Asked {
    /// Raytraced lighting (`bevy_solari`).
    pub gi: bool,
    /// Virtual geometry (meshlets).
    pub meshlets: bool,
}

impl Asked {
    /// From the arguments: `--ultra` and `--quality ultra` ask for both, `--raytraced` for the lighting and `--meshlets`
    /// for the geometry.
    pub fn parse(args: &[String]) -> Self {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let quality = args.iter().position(|a| a == "--quality").and_then(|i| args.get(i + 1));
        let both = has("--ultra") || quality.is_some_and(|q| q.trim().eq_ignore_ascii_case("ultra"));
        Asked { gi: both || has("--raytraced"), meshlets: both || has("--meshlets") }
    }

    /// The process's own arguments (natively; a browser has no Ultra tier).
    pub fn read() -> Self {
        if cfg!(target_arch = "wasm32") {
            return Asked::default();
        }
        Asked::parse(&std::env::args().collect::<Vec<_>>())
    }

    pub fn any(self) -> bool {
        self.gi || self.meshlets
    }

    fn names(self) -> &'static str {
        match (self.gi, self.meshlets) {
            (true, true) => "--ultra",
            (true, false) => "--raytraced",
            _ => "--meshlets",
        }
    }
}

/// `quality` reads `--quality` through this: `--quality ultra`, or `--ultra` with no `--quality`, holds the High level
/// (the Ultra tier is drawn on top of High). Anything else is passed on as it is.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn forced_quality(asked: String) -> String {
    let args: Vec<String> = if cfg!(target_arch = "wasm32") { vec![] } else { std::env::args().collect() };
    quality_for(asked, &args)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn quality_for(asked: String, args: &[String]) -> String {
    if asked.trim().eq_ignore_ascii_case("ultra") || (asked.trim().is_empty() && args.iter().any(|a| a == "--ultra")) {
        "high".into()
    } else {
        asked
    }
}

/// Whether this build has the Ultra tier's raytraced lighting (then it reports on `--raytraced` itself, not `visuals`).
pub fn has_raytracing() -> bool {
    cfg!(all(feature = "ultra", not(target_arch = "wasm32")))
}

/// What the tier runs with, once the GPU has been asked.
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
#[derive(Resource, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Running {
    pub gi: bool,
    pub meshlets: bool,
}

pub struct Plugin;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let asked = Asked::read();
        if !asked.any() {
            return;
        }
        #[cfg(not(all(feature = "ultra", not(target_arch = "wasm32"))))]
        {
            let _ = app;
            warn!(
                "Ultra: {} was asked for, but this build has no Ultra tier. It is native only: build it with \
                 `cargo run --release --features ultra -- --ultra` (README, \"Ultra tier (native)\"). Playing at High instead.",
                asked.names()
            );
        }
        #[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
        {
            let running = decide(asked, preflight());
            app.insert_resource(asked).insert_resource(running);
            if running.gi {
                gi::build(app);
            }
            if running.meshlets {
                geometry::build(app, running.gi);
            }
            app.add_systems(Last, report);
        }
    }
}

/// What the GPU can do for each half, before the renderer starts: `Err` with the reason it cannot.
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
#[derive(Debug, Clone)]
struct Preflight {
    gi: Result<(), String>,
    meshlets: Result<(), String>,
    adapter: String,
}

/// Asks wgpu for the high-performance adapter (as Bevy will) and checks it against what each plugin needs. A missing
/// feature is a clear message here rather than a plugin that quietly does nothing (`bevy_solari`) or exits (meshlets).
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
fn preflight() -> Preflight {
    use bevy::pbr::experimental::meshlet::MeshletPlugin;
    use bevy::solari::SolariPlugins;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..default()
    }));
    let Ok(adapter) = adapter else {
        let none = Err("no Vulkan, Metal or DX12 adapter".to_string());
        return Preflight { gi: none.clone(), meshlets: none, adapter: "none".into() };
    };
    let (info, features) = (adapter.get_info(), adapter.features());
    let check = |needed: wgpu::Features, backends: &[wgpu::Backend]| {
        let missing = needed.difference(features);
        if !backends.contains(&info.backend) {
            Err(format!("the {:?} backend (it needs {:?})", info.backend, backends))
        } else if !missing.is_empty() {
            Err(format!("GPU features {missing:?}"))
        } else {
            Ok(())
        }
    };
    Preflight {
        gi: check(SolariPlugins::required_wgpu_features(), &[wgpu::Backend::Vulkan, wgpu::Backend::Dx12]),
        meshlets: check(MeshletPlugin::required_wgpu_features(), &[wgpu::Backend::Vulkan, wgpu::Backend::Metal]),
        adapter: format!("{} ({:?}, {})", info.name, info.backend, info.driver_info),
    }
}

/// Which halves run: what was asked, what the GPU allows, and not both (Bevy 0.19.1 cannot draw meshlets for a camera with
/// a prepass, and its raytraced lighting needs one).
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
fn decide(asked: Asked, preflight: Preflight) -> Running {
    let mut running = Running { gi: false, meshlets: false };
    if asked.gi {
        match &preflight.gi {
            Ok(()) => running.gi = true,
            Err(why) => warn!("Ultra: no raytraced lighting on {}: it lacks {why}.", preflight.adapter),
        }
    }
    if asked.meshlets {
        match &preflight.meshlets {
            Ok(()) => running.meshlets = true,
            Err(why) => warn!("Ultra: no virtual geometry on {}: it lacks {why}.", preflight.adapter),
        }
    }
    if running.gi && running.meshlets {
        running.meshlets = false;
        warn!(
            "Ultra: raytraced lighting and virtual geometry cannot run together in Bevy 0.19.1: meshlets fail on any camera with \
             a prepass (their prepass binds the material at index 2, not 3), and the raytraced lighting needs one; with that \
             fixed, meshlet surfaces still reach the lighting black. Raytraced lighting only; `--meshlets` alone for the geometry."
        );
    }
    info!(
        "Ultra: {} on {}: raytraced lighting {}, virtual geometry {}",
        asked.names(),
        preflight.adapter,
        if running.gi { "on" } else { "off" },
        if running.meshlets { "on" } else { "off" }
    );
    running
}

/// Once the scene has settled, and after every new map: what the tier is drawing.
#[cfg(all(feature = "ultra", not(target_arch = "wasm32")))]
fn report(
    time: Res<Time<Real>>,
    running: Res<Running>,
    traced: Option<Res<gi::Traced>>,
    twins: Option<Res<geometry::Twins>>,
    mut last: Local<(f64, String)>,
) {
    let now = time.elapsed_secs_f64();
    if now - last.0 < 5. {
        return;
    }
    let mut line = String::new();
    if let (true, Some(traced)) = (running.gi, traced) {
        line += &traced.summary();
    }
    if let (true, Some(twins)) = (running.meshlets, twins) {
        if !line.is_empty() {
            line += "; ";
        }
        line += &twins.summary();
    }
    if line != last.1 {
        info!("Ultra: {line}");
        *last = (now, line);
    } else {
        last.0 = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        std::iter::once("hotdog").chain(line.split_whitespace()).map(String::from).collect()
    }

    #[test]
    fn the_flags_ask_for_each_half_or_both() {
        assert_eq!(Asked::parse(&args("--skip-intro")), Asked { gi: false, meshlets: false });
        assert_eq!(Asked::parse(&args("--raytraced")), Asked { gi: true, meshlets: false });
        assert_eq!(Asked::parse(&args("--meshlets --skip-intro")), Asked { gi: false, meshlets: true });
        assert_eq!(Asked::parse(&args("--ultra")), Asked { gi: true, meshlets: true });
        assert_eq!(Asked::parse(&args("--quality ultra")), Asked { gi: true, meshlets: true });
        assert_eq!(Asked::parse(&args("--quality Ultra --map assets/maps/pillow-fort.json")), Asked { gi: true, meshlets: true });
        assert_eq!(Asked::parse(&args("--quality high")), Asked { gi: false, meshlets: false });
    }

    #[test]
    fn ultra_holds_the_high_level_and_leaves_every_other_quality_alone() {
        assert_eq!(quality_for("ultra".into(), &args("--quality ultra")), "high");
        assert_eq!(quality_for("".into(), &args("--ultra")), "high");
        assert_eq!(quality_for("low".into(), &args("--ultra --quality low")), "low");
        assert_eq!(quality_for("".into(), &args("--raytraced")), "");
        assert_eq!(quality_for("medium".into(), &args("")), "medium");
    }
}

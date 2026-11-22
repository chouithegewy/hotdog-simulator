// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Checks only (`knob`, `timing=1`): where a frame's time goes, logged every two seconds: the whole frame, the main
//! world's schedules (First to Last), and the render world's three parts (up to the drawing, the drawing, after it).
use std::sync::Mutex;
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};

#[derive(Default)]
struct Marks {
    at: [Option<Instant>; 6],
    sum: [f64; 5],
    frames: u32,
    since: Option<Instant>,
}
static MARKS: Mutex<Marks> = Mutex::new(Marks { at: [None; 6], sum: [0.; 5], frames: 0, since: None });

fn mark(i: usize) {
    let Ok(mut m) = MARKS.lock() else { return };
    let now = Instant::now();
    // 0 First, 1 Last; 2 the render world's start, 3 before the drawing, 4 after it, 5 its end.
    let (from, slot) = match i {
        0 => (0, 0),
        1 => (0, 1),
        3 => (2, 2),
        4 => (3, 3),
        5 => (4, 4),
        _ => (i, 9),
    };
    if slot < 5 && let Some(then) = m.at[from] {
        m.sum[slot] += now.duration_since(then).as_secs_f64() * 1000.;
    }
    m.at[i] = Some(now);
    if i == 0 {
        m.frames += 1;
        let since = *m.since.get_or_insert(now);
        if now.duration_since(since).as_secs_f64() > 2. {
            let n = m.frames.max(1) as f64;
            info!("VR timing: frame {:.1} ms; main {:.1}; render: before the drawing {:.1}, the drawing {:.1}, after it {:.1}", m.sum[0] / n, m.sum[1] / n, m.sum[2] / n, m.sum[3] / n, m.sum[4] / n);
            (m.sum, m.frames, m.since) = ([0.; 5], 0, Some(now));
        }
    }
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        if super::knob("timing").as_deref() != Some("1") {
            return;
        }
        app.add_systems(First, || mark(0)).add_systems(Last, || mark(1));
        // Each render pass's own time, where the GPU can tell it.
        if !app.is_plugin_added::<bevy::render::diagnostic::RenderDiagnosticsPlugin>() {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        }
        app.add_systems(Last, passes);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .add_systems(Render, (|| mark(2)).before(RenderSystems::ExtractCommands))
                .add_systems(Render, (|| mark(3)).after(RenderSystems::PrepareResourcesFlush).before(RenderSystems::Render))
                .add_systems(Render, (|| mark(4)).after(RenderSystems::Render).before(RenderSystems::Cleanup))
                .add_systems(Render, (|| mark(5)).after(RenderSystems::Cleanup));
        }
    }
}

/// Logs every render pass that takes a third of a millisecond or more, every three seconds.
fn passes(store: Res<bevy::diagnostic::DiagnosticsStore>, time: Res<Time<Real>>, mut last: Local<f32>) {
    if time.elapsed_secs() - *last < 3. {
        return;
    }
    *last = time.elapsed_secs();
    let mut lines: Vec<(f64, String)> = store
        .iter()
        .filter(|d| d.path().as_str().starts_with("render/") && (d.path().as_str().ends_with("elapsed_gpu") || d.path().as_str().ends_with("elapsed_cpu")))
        .filter_map(|d| Some((d.smoothed()?, d.path().as_str().trim_start_matches("render/").to_string())))
        .filter(|(ms, _)| *ms >= 0.3)
        .collect();
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    info!("VR passes: {}", lines.iter().take(14).map(|(ms, path)| format!("{path} {ms:.1}")).collect::<Vec<_>>().join("; "));
}

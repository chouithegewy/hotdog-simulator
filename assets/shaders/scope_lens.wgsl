// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The syrup rifle optic's lens (src/scope.rs): a disc on the glass, in the first-person pass, blended (premultiplied)
// over the world the game's camera drew, magnified. Dark glass seen from the hip, clearing as the rifle comes to the
// eye; then a teal tint deepening to the edge, darker at the edge, a thin bright rim and a faint drifting reflection;
// and lit over them the reticle and the readout, drawn as signed distances a pixel or two wide, sharp at any size.
//
// The lens's own coordinates: p runs -1 to 1 across the disc, x to the right and y up as the eye sees it.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Lens {
    // rgb: the reticle's colour (linear); a: how bright it is lit.
    reticle: vec4<f32>,
    // x: how far up the rifle is (0 at the hip, 1 at the eye); yz: the reflection's drift; w: 1 on the levels that blur
    // round the lens.
    state: vec4<f32>,
    // xyz: the range readout's digits (tens, units, tenths: 0-9, 10 a dash, 11 blank); w: its brightness.
    range: vec4<f32>,
    // xy: the magazine's count in two digits; z: the burst's rounds ready; w: the burst's size.
    counts: vec4<f32>,
    // x: how far the reticle is on an enemy (0 to 1: its ring closes); the rest is unused.
    mark: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> lens: Lens;

// The amber the readout is lit in (scope.rs AMBER).
const AMBER: vec3<f32> = vec3<f32>(1.0, 0.50, 0.07);

// `colour` laid over `under` (premultiplied) with coverage `alpha`.
fn over(under: vec4<f32>, colour: vec3<f32>, alpha: f32) -> vec4<f32> {
    let a = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(colour * a + under.rgb * (1.0 - a), a + under.a * (1.0 - a));
}

// How much of a pixel `px` across a shape covers, at signed distance `d` from its edge (negative inside).
fn ink(d: f32, px: f32) -> f32 {
    return clamp(0.5 - d / px, 0.0, 1.0);
}

fn box_sdf(p: vec2<f32>, extent: vec2<f32>) -> f32 {
    let d = abs(p) - extent;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// A drop standing on its point: a round of radius `r` at the origin narrowing to a point `h` above it (an uneven
// capsule whose upper end has no radius).
fn drop_sdf(q: vec2<f32>, r: f32, h: f32) -> f32 {
    let p = vec2<f32>(abs(q.x), q.y);
    let b = r / h;
    let a = sqrt(1.0 - b * b);
    let k = dot(p, vec2<f32>(-b, a));
    if k < 0.0 {
        return length(p) - r;
    }
    if k > a * h {
        return length(p - vec2<f32>(0.0, h));
    }
    return dot(p, vec2<f32>(a, b)) - r;
}

// Which of the seven segments a code lights: a (top) b c d e f g (middle) in bits 0 to 6.
fn segments(code: i32) -> u32 {
    switch code {
        case 0: { return 0x3Fu; }
        case 1: { return 0x06u; }
        case 2: { return 0x5Bu; }
        case 3: { return 0x4Fu; }
        case 4: { return 0x66u; }
        case 5: { return 0x6Du; }
        case 6: { return 0x7Du; }
        case 7: { return 0x07u; }
        case 8: { return 0x7Fu; }
        case 9: { return 0x6Fu; }
        case 10: { return 0x40u; }
        default: { return 0u; }
    }
}

// The distance to a seven-segment digit in a cell of half size `cell` about the origin, its strokes `t` thick.
fn digit_sdf(p: vec2<f32>, code: i32, cell: vec2<f32>, t: f32) -> f32 {
    let on = segments(code);
    let w = cell.x;
    let h = cell.y;
    let across = vec2<f32>(w - t * 0.6, t * 0.5);
    let up = vec2<f32>(t * 0.5, h * 0.5 - t * 0.6);
    var d = 1000.0;
    if (on & 1u) != 0u { d = min(d, box_sdf(p - vec2<f32>(0.0, h), across)); }
    if (on & 2u) != 0u { d = min(d, box_sdf(p - vec2<f32>(w, h * 0.5), up)); }
    if (on & 4u) != 0u { d = min(d, box_sdf(p - vec2<f32>(w, -h * 0.5), up)); }
    if (on & 8u) != 0u { d = min(d, box_sdf(p - vec2<f32>(0.0, -h), across)); }
    if (on & 16u) != 0u { d = min(d, box_sdf(p - vec2<f32>(-w, -h * 0.5), up)); }
    if (on & 32u) != 0u { d = min(d, box_sdf(p - vec2<f32>(-w, h * 0.5), up)); }
    if (on & 64u) != 0u { d = min(d, box_sdf(p, across)); }
    return d;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
    let p = vec2<f32>(uv.x - 0.5, 0.5 - uv.y) * 2.0;
    let r = length(p);
    // One pixel, in the lens's radii.
    let px = max(length(fwidth(p)) * 0.7071, 1e-5);
    let aim = lens.state.x;
    let clear = smoothstep(0.3, 0.9, aim);
    let shown = smoothstep(0.7, 0.98, aim);

    var out = vec4<f32>(0.0);
    // The glass: dark from the hip, clear at the eye with a teal tint that deepens to the edge, and darker there.
    out = over(out, vec3<f32>(0.012, 0.05, 0.056), 0.92 * (1.0 - clear));
    let edge = smoothstep(0.45, 1.0, r);
    out = over(out, vec3<f32>(0.0), clear * 0.12);
    out = over(out, vec3<f32>(0.0, 0.19, 0.21), clear * (0.16 + 0.45 * edge * edge));
    out = over(out, vec3<f32>(0.0), clear * 0.6 * smoothstep(0.72, 1.0, r));
    // A faint reflection: a soft patch up and to the left that drifts against the way the view turns, and a sliver of
    // light along the upper left of the edge.
    let q = (p - vec2<f32>(-0.36, 0.44) - lens.state.yz) * vec2<f32>(1.0, 1.8);
    let glint = exp(-dot(q, q) * 7.0);
    let sliver = smoothstep(0.05, 0.0, abs(r - 0.86)) * smoothstep(0.3, 0.95, dot(p / max(r, 1e-4), vec2<f32>(-0.6, 0.8)));
    out = over(out, vec3<f32>(0.80, 0.95, 1.0), glint * mix(0.16, 0.05, clear) + sliver * mix(0.10, 0.08, clear));
    // The thin bright rim round the glass.
    out = over(out, vec3<f32>(0.62, 0.92, 0.95), mix(0.25, 0.6, clear) * ink(abs(r - (1.0 - 1.8 * px)) - 0.7 * px, px));

    if shown > 0.001 {
        let thin = max(0.9 * px, 0.0045);
        let bold = max(1.4 * px, 0.007);
        // The reticle: a syrup drop whose point is the aim; a thin ring round it broken at its four points (closed over
        // an enemy); two short stadia either side; a post below.
        var d = abs(drop_sdf(p - vec2<f32>(0.0, -0.054), 0.023, 0.054)) - thin;
        let turn = atan2(p.y, p.x);
        let gap = mix(smoothstep(0.30, 0.40, abs(sin(2.0 * turn))), 1.0, lens.mark.x);
        let ring = abs(r - 0.30) - thin;
        let stadia = min(box_sdf(vec2<f32>(abs(p.x), p.y) - vec2<f32>(0.52, 0.0), vec2<f32>(0.11, bold)), box_sdf(p - vec2<f32>(0.0, -0.48), vec2<f32>(bold, 0.08)));
        d = min(d, stadia);
        let line = max(ink(d, px), ink(ring, px) * gap);
        let halo = max(ink(d - 1.6 * px, 2.0 * px), ink(ring - 1.6 * px, 2.0 * px) * gap);
        out = over(out, vec3<f32>(0.0), 0.45 * halo * shown);
        out = over(out, lens.reticle.rgb * lens.reticle.a, line * shown);

        // The readout, in amber at the lower edge: the range in the middle (metres and a tenth), the magazine's count
        // on the left, the burst's pips on the right; each over a faint shadow, to read against a bright view.
        let cell = vec2<f32>(0.030, 0.056);
        let t = max(0.013, 1.8 * px);
        let row = -0.74;
        var digits = 1000.0;
        if i32(lens.range.x) != 11 {
            digits = min(digits, digit_sdf(p - vec2<f32>(-0.118, row), i32(lens.range.x), cell, t));
        }
        digits = min(digits, digit_sdf(p - vec2<f32>(-0.040, row), i32(lens.range.y), cell, t));
        digits = min(digits, digit_sdf(p - vec2<f32>(0.072, row), i32(lens.range.z), cell, t));
        if i32(lens.range.y) != 10 {
            digits = min(digits, length(p - vec2<f32>(0.016, row - cell.y)) - t);
        }
        var count = 1000.0;
        let low = -0.56;
        let small = cell * 0.82;
        if i32(lens.counts.x) != 11 {
            count = min(count, digit_sdf(p - vec2<f32>(-0.585, low), i32(lens.counts.x), small, t * 0.9));
        }
        count = min(count, digit_sdf(p - vec2<f32>(-0.515, low), i32(lens.counts.y), small, t * 0.9));
        let size = i32(lens.counts.w);
        var pips = 1000.0;
        for (var i = 0; i < 6; i = i + 1) {
            if i >= size {
                break;
            }
            let at = p - vec2<f32>(0.46 + 0.068 * f32(i), low);
            let pip = length(at) - 0.024;
            // A round still to come is a filled pip; one gone, an empty ring.
            if f32(i) < lens.counts.z {
                pips = min(pips, pip);
            } else {
                pips = min(pips, abs(pip) - thin);
            }
        }
        let marks = min(count, pips);
        out = over(out, vec3<f32>(0.0), 0.4 * max(ink(digits - 1.6 * px, 2.0 * px) * lens.range.w, ink(marks - 1.6 * px, 2.0 * px)) * shown);
        out = over(out, AMBER * lens.reticle.a, 0.95 * ink(digits, px) * lens.range.w * shown);
        out = over(out, AMBER * lens.reticle.a, 0.9 * ink(marks, px) * shown);
    }

#ifdef TONEMAP_IN_SHADER
    if out.a > 1e-4 {
        let straight = tone_mapping(vec4<f32>(out.rgb / out.a, 1.0), view.color_grading).rgb;
        out = vec4<f32>(straight * out.a, out.a);
    }
#endif
    return out;
}

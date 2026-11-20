// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// Sharpening for a picture that the page is about to stretch to the screen (src/quality.rs, the render scale): the
// stretch blurs every edge across a pixel or two, so each pixel is first pushed away from the average of its four
// neighbours. Never past the lightest and darkest of the five, so edges come out crisp without the bright and dark
// halos of a plain unsharp mask, and flat areas, where the five agree, are left as they are.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Sharpening {
    // x: how hard, 0 to 1; the rest is unused (WebGL 2 binds uniforms in multiples of 16 bytes).
    settings: vec4<f32>,
};

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> sharpening: Sharpening;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let e = textureSample(picture, picture_sampler, in.uv);
    let b = textureSample(picture, picture_sampler, in.uv, vec2<i32>(0, -1)).rgb;
    let d = textureSample(picture, picture_sampler, in.uv, vec2<i32>(-1, 0)).rgb;
    let f = textureSample(picture, picture_sampler, in.uv, vec2<i32>(1, 0)).rgb;
    let h = textureSample(picture, picture_sampler, in.uv, vec2<i32>(0, 1)).rgb;
    let lo = min(min(min(b, d), min(f, h)), e.rgb);
    let hi = max(max(max(b, d), max(f, h)), e.rgb);
    let detail = e.rgb - 0.25 * (b + d + f + h);
    let sharp = e.rgb + detail * (2.5 * sharpening.settings.x);
    return vec4<f32>(clamp(sharp, lo, hi), e.a);
}

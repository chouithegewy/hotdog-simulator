// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The blur round the syrup rifle's optic (src/scope/blur.rs): one of two passes, across the picture, or down it with
// DOWN. Within the circle the picture is left as it is; beyond it the blur's radius comes up over the feather to its
// full size, and seventeen taps a pixel along the pass's way are weighed as a bell curve over that radius.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Blur {
    // xy: the circle's middle on the picture (0 to 1); z: its radius and w: the feather, as shares of the height.
    circle: vec4<f32>,
    // x: the blur's radius at its fullest, a share of the height; the rest is unused (WebGL 2 binds uniforms in
    // multiples of 16 bytes).
    size: vec4<f32>,
};

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> blur: Blur;

const TAPS: i32 = 8;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(picture));
    let from_middle = (in.uv - blur.circle.xy) * vec2<f32>(size.x / size.y, 1.0);
    let reach = smoothstep(blur.circle.z, blur.circle.z + blur.circle.w, length(from_middle));
    // The blur's radius here, in pixels.
    let radius = reach * blur.size.x * size.y;
    let here = textureSampleLevel(picture, picture_sampler, in.uv, 0.0);
    if radius < 0.5 {
        return here;
    }
#ifdef DOWN
    let way = vec2<f32>(0.0, 1.0 / size.y);
#else
    let way = vec2<f32>(1.0 / size.x, 0.0);
#endif
    let stride = radius / f32(TAPS);
    let spread = radius * 0.5;
    var sum = here;
    var weight = 1.0;
    for (var i = 1; i <= TAPS; i = i + 1) {
        let x = f32(i) * stride;
        let w = exp(-0.5 * x * x / (spread * spread));
        sum = sum + (textureSampleLevel(picture, picture_sampler, in.uv + way * x, 0.0) + textureSampleLevel(picture, picture_sampler, in.uv - way * x, 0.0)) * w;
        weight = weight + 2.0 * w;
    }
    return sum / weight;
}

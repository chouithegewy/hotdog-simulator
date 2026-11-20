// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The darkness round the syrup rifle optic's lens (src/scope.rs): a quad in the glass's plane in the first-person
// pass, with a hole the glass's size, darkening the view outside the ring softly and more towards the edges of the
// screen.
#import bevy_pbr::forward_io::VertexOutput

struct Veil {
    // x: how dark just outside the ring; y: how dark at the edges of the view; z: the ring's radius in the lens's
    // radii; w: how far the veil is up (0 to 1).
    shade: vec4<f32>,
    // x: the quad's half size in the lens's radii; the rest is unused (WebGL 2 binds uniforms in multiples of 16 bytes).
    size: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> veil: Veil;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
    let p = (uv - vec2<f32>(0.5)) * 2.0 * veil.size.x;
    let r = length(p);
    let px = max(length(fwidth(p)) * 0.7071, 1e-5);
    let hole = clamp((r - 1.0) / px + 0.5, 0.0, 1.0);
    let ring = veil.shade.z;
    let far = smoothstep(ring, ring * 3.4, r);
    // (Faded out towards the quad's own edges, which never show sharp.)
    let rim = 1.0 - smoothstep(0.7, 1.0, max(abs(p.x), abs(p.y)) / veil.size.x);
    let dark = mix(veil.shade.x, veil.shade.y, far) * veil.shade.w * hole * rim;
    return vec4<f32>(0.0, 0.0, 0.0, dark);
}

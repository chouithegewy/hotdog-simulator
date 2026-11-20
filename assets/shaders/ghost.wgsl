// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The placement preview (src/preview.rs): a see-through, unlit tint whose edges glow. Where a surface turns away
// from the eye it is more opaque and paler (its rim, the soft outline of its silhouette), and so is it where the
// surface bends sharply (the rounded edges and corners of a box, found by how fast its normal turns from one pixel to
// the next), so that a shape reads by its outline rather than by a flat fill. It is drawn over the world (the
// first-person pass, depth test off), so those are the cues to its shape.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
}

struct Ghost {
    // rgb: the tint (linear); a: how opaque it is where it faces the eye.
    colour: vec4<f32>,
    // x: how much more opaque the rim is; y: how sharply the rim falls off (a power); z: how far the whole breathes
    // (0 steady); w: how much paler the rim is.
    rim: vec4<f32>,
    // x: how much more opaque a bend is; y: how fast the normal must turn across a pixel to count fully as one.
    crease: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> ghost: Ghost;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    let towards = normalize(view.world_position - in.world_position.xyz);
    let edge = pow(1.0 - clamp(abs(dot(normal, towards)), 0.0, 1.0), max(ghost.rim.y, 0.1));
    let turn = length(fwidth(in.world_normal));
    let bend = smoothstep(0.25 * ghost.crease.y, ghost.crease.y, turn);
    let breath = 1.0 - ghost.rim.z * (0.5 + 0.5 * sin(globals.time * 4.0));
    let alpha = clamp((ghost.colour.a + ghost.rim.x * edge + ghost.crease.x * bend) * breath, 0.0, 1.0);
    let tint = mix(ghost.colour.rgb, vec3<f32>(1.0), ghost.rim.w * max(edge, bend));
    return vec4<f32>(tint, alpha);
}

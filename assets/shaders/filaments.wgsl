// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// Thousands of additive light filaments drawn from one mesh: sun rays, corona spikes, glints on the
// counter and dust motes. Every filament is four vertices that the vertex stage widens into a ribbon.
//
//   position  where the filament starts
//   normal    the vector from its start to its end (not a unit vector)
//   uv        x: 0 at the start and 1 at the end, y: 0 and 1 across the width
//   color     x: random seed, y: width in metres, z: brightness, w: shape
//             shape 0 is a ribbon turned to face the camera around its own axis (a ray)
//             shape 1 is a round sprite facing the camera (a mote, a bulb's glow)
//             shape 2 is a ribbon lying flat in the horizontal plane (a glint on the counter)
#import bevy_pbr::{
    mesh_functions,
    forward_io::Vertex,
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{view, globals},
}

struct Filaments {
    // rgb tint, a overall intensity
    color: vec4<f32>,
    // x shimmer rate, y shimmer depth (0 steady, 1 blinks fully off), z drift distance in metres, w drift rate
    motion: vec4<f32>,
    // x, y: filaments fade out between these distances as they approach the camera
    // z: how much a ray thins out toward its far end, w: narrowest a ribbon may get, in pixels
    fade: vec4<f32>,
    // x: widest a ribbon may get, in pixels (0 for no limit)
    limits: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> filaments: Filaments;

struct Ribbon {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) glow: vec4<f32>,
}

@vertex
fn vertex(in: Vertex) -> Ribbon {
    var out: Ribbon;
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    let seed = in.color.x;
    let shape = in.color.w;
    let time = globals.time;
    let turn = seed * 6.2831853;

    let rate = filaments.motion.w;
    let drift = filaments.motion.z * vec3<f32>(
        sin(time * rate * (0.6 + seed) + turn * 3.0),
        sin(time * rate * 0.7 * (0.5 + fract(seed * 7.3)) + turn * 5.0) * 0.6,
        cos(time * rate * (0.8 + fract(seed * 3.1)) + turn * 2.0),
    );
    let start = (world_from_local * vec4<f32>(in.position, 1.0)).xyz + drift;
    let reach = (world_from_local * vec4<f32>(in.normal, 0.0)).xyz;
    let along = reach / max(length(reach), 0.00001);

    let centre = start + reach * in.uv.x;
    let to_camera = view.world_position - centre;
    let distance = max(length(to_camera), 0.00001);
    let facing = to_camera / distance;

    // A ribbon thinner than a pixel is drawn a pixel wide and dimmed by the same factor, so hair-thin
    // rays stay continuous lines instead of breaking into crawling dots.
    let pixel = 2.0 * distance / (view.clip_from_view[1][1] * view.viewport.w);
    // One held to a few pixels at most stays a hair however close the camera comes.
    let widest = select(filaments.limits.x, 100000.0, filaments.limits.x <= 0.0);
    let width = clamp(in.color.y, pixel * filaments.fade.w, pixel * widest);
    let coverage = min(in.color.y / width, 1.0);

    var across = cross(along, facing);
    var edge_on = 1.0;
    var position = centre;
    if shape > 1.5 {
        across = cross(along, vec3<f32>(0.0, 1.0, 0.0));
        position = centre + normalize(across) * (in.uv.y - 0.5) * width;
        // A glint is brightest where the surface mirrors the light toward the eye: seen from low down.
        edge_on = 0.35 + 0.65 * (1.0 - abs(facing.y));
    } else if shape > 0.5 {
        let right = view.world_from_view[0].xyz;
        let up = view.world_from_view[1].xyz;
        position = start + (right * (in.uv.x - 0.5) + up * (in.uv.y - 0.5)) * width;
    } else {
        let slant = length(across);
        position = centre + across / max(slant, 0.0001) * (in.uv.y - 0.5) * width;
        // A ray seen straight down its own length has no width to draw: let it fade instead of flicker.
        edge_on = smoothstep(0.03, 0.3, slant);
    }
    out.position = position_world_to_clip(position);

    let depth = filaments.motion.y;
    let quick = 0.5 + 0.5 * sin(time * filaments.motion.x * (0.5 + fract(seed * 13.7)) + turn * 7.0);
    let slow = 0.5 + 0.5 * sin(time * filaments.motion.x * 0.23 * (0.6 + fract(seed * 5.9)) + turn * 11.0);
    let shimmer = (1.0 - depth * quick * quick) * (1.0 - 0.5 * depth * slow);
    let near = smoothstep(filaments.fade.x, filaments.fade.y, distance);
    let strength = filaments.color.a * in.color.z * coverage * shimmer * near * edge_on;
    out.glow = vec4<f32>(filaments.color.rgb * strength, shape);
    out.uv = in.uv;
    return out;
}

@fragment
fn fragment(in: Ribbon) -> @location(0) vec4<f32> {
    var shade = 0.0;
    if in.glow.a > 0.5 && in.glow.a < 1.5 {
        let offset = in.uv * 2.0 - vec2<f32>(1.0);
        let falloff = max(1.0 - dot(offset, offset), 0.0);
        shade = falloff * falloff;
    } else {
        let side = 1.0 - abs(in.uv.y * 2.0 - 1.0);
        let soft = side * side * (3.0 - 2.0 * side);
        let t = in.uv.x;
        let taper = mix(1.0, (1.0 - t) * (1.0 - t), filaments.fade.z);
        shade = soft * smoothstep(0.0, 0.05, t) * (1.0 - smoothstep(0.8, 1.0, t)) * taper;
    }
    // Additive: the pipeline blends premultiplied colour, and an alpha of zero keeps what is behind.
    return vec4<f32>(in.glow.rgb * shade, 0.0);
}

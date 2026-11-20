// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The glow round the hotdog under the syrup rifle optic's reticle (src/scope.rs): a shell over its sausage and buns,
// pushed out along their normals (further the further it is from the eye, so that the glow keeps about its width on
// the screen) and added to the picture. Brightest where the shell is seen edge-on just outside the hotdog's own
// outline, fading out to the shell's edge and over the hotdog's middle: a soft rim. It is drawn in the world's pass
// with the depth test, so whatever stands in front of the hotdog hides it.
#import bevy_pbr::{
    mesh_functions,
    forward_io::Vertex,
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
}
// A shell over a part bent by the nod bends with it: the nod rides in its tag as in the part's (player_model/bend.rs).
#import hotdog::bend::{bend, nod_of, turn_at, turn_x}

struct Glow {
    // rgb: the colour (linear); a: how far it is up, 0 to 1.
    colour: vec4<f32>,
    // x: how far the shell stands off the surface, metres; y: how much further a metre from the eye; z: how bright the
    // rim is; w: unused.
    shape: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> glow: Glow;

struct Shell {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world: vec3<f32>,
}

@vertex
fn vertex(in: Vertex) -> Shell {
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    let nod = nod_of(mesh_functions::get_tag(in.instance_index));
    let at = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(bend(nod, in.position), 1.0)).xyz;
#ifdef VERTEX_NORMALS
    let n = normalize(mesh_functions::mesh_normal_local_to_world(turn_x(turn_at(nod, in.position.y), in.normal), in.instance_index));
#else
    let n = vec3<f32>(0.0, 1.0, 0.0);
#endif
    let far = distance(at, view.world_position);
    let world = at + n * (glow.shape.x + glow.shape.y * far);
    var out: Shell;
    out.position = position_world_to_clip(world);
    out.normal = n;
    out.world = world;
    return out;
}

@fragment
fn fragment(in: Shell) -> @location(0) vec4<f32> {
    let towards = normalize(view.world_position - in.world);
    let facing = abs(dot(normalize(in.normal), towards));
    let band = smoothstep(0.0, 0.42, facing) * (1.0 - smoothstep(0.5, 0.92, facing));
    let light = glow.colour.rgb * glow.colour.a * (band * glow.shape.z + 0.05);
    return vec4<f32>(light, 0.0);
}

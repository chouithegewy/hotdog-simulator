// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// A hotdog's sausage, buns and mustard, bent by the nod in their mesh's tag (src/player_model/bend.rs): Bevy's mesh
// vertex shader (bevy_pbr mesh.wgsl, without skins and morph targets, which these meshes have not) with the bend put on
// the position, the normal and the tangent before they go into the world. The fragment shader is the standard one.
#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
#import hotdog::bend::{bend, nod_of, turn_at, turn_x}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let nod = nod_of(mesh_functions::get_tag(vertex.instance_index));

#ifdef VERTEX_POSITIONS
    let turn = turn_at(nod, vertex.position.y);
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turn_x(turn, vertex.normal), vertex.instance_index);
#endif
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(bend(nod, vertex.position), 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vec4<f32>(turn_x(turn, vertex.tangent.xyz), vertex.tangent.w),
        vertex.instance_index
    );
#endif
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}

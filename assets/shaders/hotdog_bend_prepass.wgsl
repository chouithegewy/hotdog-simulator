// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The depth, normal, motion and deferred prepasses and the shadow maps of a hotdog's bent sausage, buns and mustard
// (src/player_model/bend.rs): Bevy's prepass vertex shader (bevy_pbr prepass.wgsl, without skins and morph targets)
// with the bend in the mesh's tag put on the position, the normal and the tangent. Its last frame's place is taken
// with this frame's nod (a nod is eased, so the difference is a hair's).
#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
#import hotdog::bend::{bend, nod_of, turn_at, turn_x}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let nod = nod_of(mesh_functions::get_tag(vertex.instance_index));
    let local = bend(nod, vertex.position);

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    let turn = turn_at(nod, vertex.position.y);
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turn_x(turn, vertex.normal), vertex.instance_index);
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vec4<f32>(turn_x(turn, vertex.tangent.xyz), vertex.tangent.w),
        vertex.instance_index
    );
#endif
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef MOTION_VECTOR_PREPASS
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_previous_world_from_local(vertex.instance_index),
        vec4<f32>(local, 1.0)
    );
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}

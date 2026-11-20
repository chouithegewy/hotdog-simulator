// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The titan's casing and crust (src/titan/rig.rs `Embers`): the standard material, lit as usual, with two glows added
// after the lighting so that the giant carries its own light into a dark kitchen. Its embers: where the vertex colour's
// alpha says (its splits, its grill-mark scars, the charred edges of its bun), a deep orange-red that breathes, flares
// with every wind-up and with a hit on its weak spot, and dies to a dull red as it falls. Its rim: a faint fresnel glow
// along its outline, so the whole giant reads against the dark from any side. Nothing here casts or receives light.
#import bevy_pbr::{
    pbr_functions::alpha_discard,
    pbr_fragment::pbr_input_from_standard_material,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

struct Embers {
    // rgb: the embers' colour times how hot they are now; a: unused.
    glow: vec4<f32>,
    // rgb: the rim's colour times its strength; a: how sharply it falls off from the outline.
    rim: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> embers: Embers;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // (The embers' mask rides in the vertex colour's alpha: the surface itself is opaque.)
    var mask = 0.0;
#ifdef VERTEX_COLORS
    mask = in.color.a;
#endif
    pbr_input.material.base_color.a = 1.0;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    let facing = clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0);
    let rim = pow(1.0 - facing, max(embers.rim.a, 0.5));
    out.color = vec4<f32>(out.color.rgb + embers.glow.rgb * mask + embers.rim.rgb * rim, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}

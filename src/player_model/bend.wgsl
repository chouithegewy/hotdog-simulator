// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The nod drawn (src/player_model/bend.rs): the bend kitchen_sim's head.rs puts a hotdog's head through, in a standing
// hotdog's frame (heights above its feet, x to its right, its face on -z). The body below NECK_LOW keeps still, the head
// above NECK_HIGH turns as one, and the axis between curves smoothly, each point carried round with it; bowing, the neck
// gives SQUASH of its length at a full BOW. The nod (radians about x, negative bowing) rides in the mesh's tag, the bits
// of an f32 (bend.rs `tag`); a tag of 0 is no nod. These numbers are head.rs's (a test holds them to it).
#define_import_path hotdog::bend

const NECK_LOW: f32 = 0.09;
const NECK_HIGH: f32 = 0.12;
const BOW: f32 = 0.7853982;
const SQUASH: f32 = 0.2;

fn nod_of(tag: u32) -> f32 {
    return bitcast<f32>(tag);
}

// sin(x)/x and (1 - cos x)/x, smooth through 0.
fn sinc(x: f32) -> f32 {
    if abs(x) < 1e-3 {
        return 1.0 - x * x / 6.0;
    }
    return sin(x) / x;
}
fn cosc(x: f32) -> f32 {
    if abs(x) < 1e-3 {
        return x / 2.0 - x * x * x / 24.0;
    }
    return (1.0 - cos(x)) / x;
}

// `v` turned by `a` about x.
fn turn_x(a: f32, v: vec3<f32>) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(v.x, v.y * c - v.z * s, v.y * s + v.z * c);
}

// How much the bend turns what is at height `y` (its normals and tangents turn by it).
fn turn_at(nod: f32, y: f32) -> f32 {
    return nod * clamp((y - NECK_LOW) / (NECK_HIGH - NECK_LOW), 0.0, 1.0);
}

// Where the bend takes `p` (head.rs `bend`).
fn bend(nod: f32, p: vec3<f32>) -> vec3<f32> {
    if p.y <= NECK_LOW || nod == 0.0 {
        return p;
    }
    let along = min(p.y, NECK_HIGH) - NECK_LOW;
    let turn = nod * along / (NECK_HIGH - NECK_LOW);
    let length = (1.0 - SQUASH * min(max(-nod, 0.0) / BOW, 1.0)) * along;
    return vec3<f32>(0.0, NECK_LOW + length * sinc(turn), length * cosc(turn))
        + turn_x(turn, vec3<f32>(p.x, max(p.y - NECK_HIGH, 0.0), p.z));
}

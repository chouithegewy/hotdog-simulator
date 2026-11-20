// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The titan's entities: one root (its feet, its yaw, scaled to the simulation's size) and a flat rig of parts under
//! it, each placed by the solved skeleton (pose.rs) every frame. Twenty-three mesh entities, seven shared materials and,
//! at High and Medium only, one small unshadowed light; the meshes are made once for the graphics level and made again
//! in the same handles when the level changes.
//!
//! **It carries its own light**, for a kitchen at night: its casing and its crust are drawn in [`TitanMaterial`], the
//! standard material with embers and a rim added (assets/shaders/titan_casing.wgsl): the splits, the grill-mark scars
//! and the bun's charred edges glow like embers where their vertex colours' alpha says, as hot as [`Heat`] says this
//! frame, and a faint dark-red fresnel rim keeps its outline against the dark from every side. Its furnace grin, its eyes
//! and its weak spot's patch are unlit and glow on their own; at High and Medium the furnace also lights the floor and
//! its own chest and hands a little.
use super::body::{self, Grip, Lod, Shapes, GRIPS};
use super::pose::{L, R, Skeleton};
use bevy::light::NotShadowCaster;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// The embers and the rim laid over the standard material (titan_casing.wgsl).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct Embers {
    /// The embers' colour times how hot they are now (rgb).
    #[uniform(100)]
    pub glow: Vec4,
    /// The rim's colour times its strength (rgb), and how sharply it falls off from the outline (a).
    #[uniform(100)]
    pub rim: Vec4,
}
impl MaterialExtension for Embers {
    fn fragment_shader() -> ShaderRef {
        "shaders/titan_casing.wgsl".into()
    }
}
/// The titan's casing and crust.
pub type TitanMaterial = ExtendedMaterial<StandardMaterial, Embers>;

/// How the giant glows this frame (fx.rs works it out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heat {
    /// The embers: 1 its usual smoulder, more in a wind-up, hurt, flaring on a weak-spot hit; 0 out.
    pub embers: f32,
    /// How far they have died to a dull red (0 to 1: it is falling, or down).
    pub dull: f32,
    /// The furnace in its grin, its eyes, and its weak spot's patch (1 the usual).
    pub maw: f32,
    pub eyes: f32,
    pub spot: f32,
}
/// The embers' colour at their hottest, and dull; the rim's.
const EMBER: [f32; 3] = [1.0, 0.27, 0.045];
const DULL: [f32; 3] = [0.45, 0.04, 0.01];
const RIM: [f32; 3] = [0.27, 0.06, 0.025];
/// How sharply the rim falls off from the outline.
const RIM_POWER: f32 = 3.0;
/// The cold of the weak spot's patch (linear).
const COLD: [f32; 3] = [0.55, 0.72, 1.0];

/// The meshes and materials every titan is drawn with.
#[derive(Resource, Clone)]
pub struct Kit {
    pub lod: Lod,
    pub torso: Handle<Mesh>,
    pub head: Handle<Mesh>,
    pub mouth: Handle<Mesh>,
    pub maw: Handle<Mesh>,
    pub eyes: [Handle<Mesh>; 2],
    pub bun: Handle<Mesh>,
    pub upper: [Handle<Mesh>; 2],
    pub fore: [Handle<Mesh>; 2],
    pub hands: [[Handle<Mesh>; 2]; 2],
    pub thighs: [Handle<Mesh>; 2],
    pub shins: [Handle<Mesh>; 2],
    pub feet: [Handle<Mesh>; 2],
    pub pick: Handle<Mesh>,
    pub patch: Handle<Mesh>,
    /// The casing (vertex-coloured, a greasy sheen, embers and a rim), the crust (embers at its edges, a rim), the
    /// enamel of the teeth (wet), the eyes (pale, lit from nowhere: they catch the eye in the dark), the ember glowing
    /// inside the mouth, the weak spot's patch (a cold glow) and its mark (its pick and pennant; it pulses).
    pub casing: Handle<TitanMaterial>,
    pub crust: Handle<TitanMaterial>,
    pub spot: Handle<StandardMaterial>,
    pub enamel: Handle<StandardMaterial>,
    pub eye: Handle<StandardMaterial>,
    pub ember: Handle<StandardMaterial>,
    pub mark: Handle<StandardMaterial>,
    /// Triangles drawn for one titan at this level.
    pub triangles: usize,
}
/// Each side's shape and its mirror image.
fn sided(meshes: &mut Assets<Mesh>, right: &super::shape::Shape) -> [Handle<Mesh>; 2] {
    [meshes.add(right.clone().mirrored().mesh()), meshes.add(right.mesh())]
}
fn resided(meshes: &mut Assets<Mesh>, handles: &[Handle<Mesh>; 2], right: &super::shape::Shape) {
    let _ = meshes.insert(&handles[L], right.clone().mirrored().mesh());
    let _ = meshes.insert(&handles[R], right.mesh());
}
impl Kit {
    /// The kit made in `world` (its meshes and its materials' assets).
    pub fn make(world: &mut World, lod: Lod) -> Kit {
        // (A world without the renderer's materials, a check's, gets their assets here.)
        if !world.contains_resource::<Assets<TitanMaterial>>() {
            world.init_resource::<Assets<TitanMaterial>>();
        }
        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|world, mut titan: Mut<Assets<TitanMaterial>>| Kit::new(&mut meshes, &mut world.resource_mut::<Assets<StandardMaterial>>(), &mut titan, lod))
        })
    }
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, titan: &mut Assets<TitanMaterial>, lod: Lod) -> Kit {
        let s = Shapes::new(lod);
        let white = |roughness: f32, clearcoat: f32| StandardMaterial { base_color: Color::WHITE, perceptual_roughness: roughness, clearcoat, clearcoat_perceptual_roughness: 0.35, reflectance: 0.45, ..default() };
        let glowing = |base: StandardMaterial| ExtendedMaterial { base, extension: Embers { glow: Vec4::ZERO, rim: Vec4::new(RIM[0], RIM[1], RIM[2], RIM_POWER) } };
        Kit {
            lod,
            torso: meshes.add(s.torso.mesh()),
            head: meshes.add(s.head.mesh()),
            mouth: meshes.add(s.mouth.mesh()),
            maw: meshes.add(s.maw.mesh()),
            eyes: sided(meshes, &s.eye),
            bun: meshes.add(s.bun.mesh()),
            upper: sided(meshes, &s.upper),
            fore: sided(meshes, &s.fore),
            hands: [sided(meshes, &s.hands[0]), sided(meshes, &s.hands[1])],
            thighs: sided(meshes, &s.thigh),
            shins: sided(meshes, &s.shin),
            feet: sided(meshes, &s.foot),
            pick: meshes.add(s.pick.mesh()),
            patch: meshes.add(s.patch.mesh()),
            casing: titan.add(glowing(white(0.42, 0.25))),
            crust: titan.add(glowing(white(0.78, 0.))),
            spot: materials.add(StandardMaterial { base_color: Color::linear_rgb(COLD[0], COLD[1], COLD[2]), unlit: true, ..default() }),
            enamel: materials.add(white(0.22, 0.6)),
            eye: materials.add(StandardMaterial { base_color: Color::linear_rgb(1.2, 1.15, 1.0), unlit: true, ..default() }),
            ember: materials.add(StandardMaterial { base_color: Color::linear_rgb(1.7, 1.7, 1.7), unlit: true, ..default() }),
            mark: materials.add(StandardMaterial { emissive: LinearRgba::rgb(0.3, 0.3, 0.32), ..white(0.5, 0.) }),
            triangles: s.triangles(),
        }
    }
    /// Makes every mesh again, finer or coarser, in the same handles (the entities keep them).
    pub fn remake(&mut self, meshes: &mut Assets<Mesh>, lod: Lod) {
        if lod == self.lod {
            return;
        }
        let s = Shapes::new(lod);
        let _ = meshes.insert(&self.torso, s.torso.mesh());
        let _ = meshes.insert(&self.head, s.head.mesh());
        let _ = meshes.insert(&self.mouth, s.mouth.mesh());
        let _ = meshes.insert(&self.maw, s.maw.mesh());
        let _ = meshes.insert(&self.bun, s.bun.mesh());
        let _ = meshes.insert(&self.pick, s.pick.mesh());
        let _ = meshes.insert(&self.patch, s.patch.mesh());
        resided(meshes, &self.eyes, &s.eye);
        resided(meshes, &self.upper, &s.upper);
        resided(meshes, &self.fore, &s.fore);
        for (k, shape) in s.hands.iter().enumerate() {
            resided(meshes, &self.hands[k], shape);
        }
        resided(meshes, &self.thighs, &s.thigh);
        resided(meshes, &self.shins, &s.shin);
        resided(meshes, &self.feet, &s.foot);
        self.lod = lod;
        self.triangles = s.triangles();
    }
    /// The hand mesh for a grip.
    pub fn hand(&self, side: usize, grip: Grip) -> &Handle<Mesh> {
        &self.hands[GRIPS.iter().position(|g| *g == grip).unwrap_or(0)][side]
    }
}

/// A titan drawn: on its root.
#[derive(Component, Clone)]
pub struct Rig {
    /// The hotdog it is (its player's id).
    #[allow(dead_code)]
    pub id: u32,
    pub torso: Entity,
    pub head: Entity,
    pub eyes: [Entity; 2],
    pub upper: [Entity; 2],
    pub fore: [Entity; 2],
    /// Each wrist, and the hand shapes on it (`GRIPS`).
    pub wrists: [Entity; 2],
    pub hands: [[Entity; 2]; 2],
    pub thighs: [Entity; 2],
    pub shins: [Entity; 2],
    pub feet: [Entity; 2],
    /// Which grip each hand shows.
    pub grips: [Grip; 2],
    /// Whether it is drawn as its own player sees it in first person (its arms and legs only).
    pub own: bool,
    /// The light of the furnace in its mouth.
    pub glow: Entity,
}
impl Rig {
    /// Every part (not the root), for the checks.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn parts(&self) -> Vec<Entity> {
        let mut v = vec![self.torso, self.head];
        for i in [L, R] {
            v.extend([self.eyes[i], self.upper[i], self.fore[i], self.wrists[i], self.hands[i][0], self.hands[i][1], self.thighs[i], self.shins[i], self.feet[i]]);
        }
        v
    }
    /// The parts its own player does not see in first person (the camera is in its head).
    pub fn unseen(&self) -> Vec<Entity> {
        vec![self.torso, self.head, self.eyes[L], self.eyes[R], self.thighs[L], self.thighs[R]]
    }
}

/// The maw light's brightness (lumens).
pub const MAW_LIGHT: f32 = 2600.;
/// How big the titan's root is drawn: the simulation's height over the meshes'.
pub fn scale() -> f32 {
    kitchen_sim::titan::TALL / body::REFERENCE
}

/// Makes a titan for hotdog `id` at `at`, facing `yaw`.
pub fn spawn(world: &mut World, kit: &Kit, id: u32, at: Vec3, yaw: f32) -> Entity {
    let root = world.spawn((Name::new("Titan"), Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(scale())), Visibility::Visible)).id();
    let plain = |world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, shadow: bool| part(world, parent, mesh, MeshMaterial3d(material.clone()), shadow);
    let casing = |world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: &Handle<TitanMaterial>| part(world, parent, mesh, MeshMaterial3d(material.clone()), true);
    let torso = casing(world, root, &kit.torso, &kit.casing);
    // The bun rides on the torso; the mouth and the weak spot's pick and patch on the head.
    let bun = casing(world, torso, &kit.bun, &kit.crust);
    world.entity_mut(bun).insert(Name::new("Titan's bun armour"));
    let head = casing(world, root, &kit.head, &kit.casing);
    let patch = plain(world, head, &kit.patch, &kit.spot, false);
    world.entity_mut(patch).insert(Name::new("Titan's weak spot patch"));
    let mouth = plain(world, head, &kit.mouth, &kit.enamel, false);
    let pick = plain(world, head, &kit.pick, &kit.mark, false);
    let maw = plain(world, head, &kit.maw, &kit.ember, false);
    world.entity_mut(maw).insert(Name::new("Titan's maw"));
    // The furnace in its mouth lights the floor and the small hotdogs in front of it, a little (not at Low or Lowest).
    let glow = world
        .spawn((
            Name::new("Titan's maw light"),
            PointLight { intensity: MAW_LIGHT, range: 2.6, radius: 0.08, color: Color::srgb(1., 0.5, 0.18), shadow_maps_enabled: false, ..default() },
            Transform::from_translation(Vec3::new(0., body::GRIN.0, -0.34)),
            if kit.lod == Lod::Full { Visibility::Inherited } else { Visibility::Hidden },
        ))
        .id();
    world.entity_mut(head).add_child(glow);
    world.entity_mut(mouth).insert(Name::new("Titan's grin"));
    world.entity_mut(pick).insert(Name::new("Titan's weak spot"));
    let mut rig = Rig {
        id,
        torso,
        head,
        eyes: [Entity::PLACEHOLDER; 2],
        upper: [Entity::PLACEHOLDER; 2],
        fore: [Entity::PLACEHOLDER; 2],
        wrists: [Entity::PLACEHOLDER; 2],
        hands: [[Entity::PLACEHOLDER; 2]; 2],
        thighs: [Entity::PLACEHOLDER; 2],
        shins: [Entity::PLACEHOLDER; 2],
        feet: [Entity::PLACEHOLDER; 2],
        grips: [Grip::Open; 2],
        own: false,
        glow,
    };
    for i in [L, R] {
        rig.eyes[i] = plain(world, root, &kit.eyes[i], &kit.eye, false);
        rig.upper[i] = casing(world, root, &kit.upper[i], &kit.casing);
        rig.fore[i] = casing(world, root, &kit.fore[i], &kit.casing);
        let wrist = world.spawn((Transform::default(), Visibility::Inherited)).id();
        world.entity_mut(root).add_child(wrist);
        rig.wrists[i] = wrist;
        for (k, grip) in GRIPS.iter().enumerate() {
            let hand = casing(world, wrist, kit.hand(i, *grip), &kit.casing);
            world.entity_mut(hand).insert(if *grip == Grip::Open { Visibility::Inherited } else { Visibility::Hidden });
            rig.hands[i][k] = hand;
        }
        rig.thighs[i] = casing(world, root, &kit.thighs[i], &kit.casing);
        rig.shins[i] = casing(world, root, &kit.shins[i], &kit.casing);
        rig.feet[i] = casing(world, root, &kit.feet[i], &kit.casing);
    }
    world.entity_mut(root).insert(rig);
    root
}

/// One part of the rig: a mesh in a material, under `parent`, casting a shadow or not.
fn part<M: Material>(world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: MeshMaterial3d<M>, shadow: bool) -> Entity {
    // (Its own materials, as made: the scenery's dressing, visuals/surfaces.rs, leaves it alone.)
    let mut e = world.spawn((Mesh3d(mesh.clone()), material, Transform::default(), Visibility::Inherited, crate::visuals::Surveyed));
    if !shadow {
        e.insert(NotShadowCaster);
    }
    let e = e.id();
    world.entity_mut(parent).add_child(e);
    e
}

fn place(world: &mut World, e: Entity, t: Transform) {
    if let Some(mut tr) = world.get_mut::<Transform>(e) {
        if *tr != t {
            *tr = t;
        }
    }
}
fn show(world: &mut World, e: Entity, on: bool) {
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if world.get::<Visibility>(e).is_some_and(|v| *v != want) {
        world.entity_mut(e).insert(want);
    }
}
/// Puts the rig on `root` in the skeleton's pose.
pub fn pose(world: &mut World, root: Entity, s: &Skeleton) {
    let Some(mut rig) = world.get::<Rig>(root).cloned() else {
        return;
    };
    place(world, rig.torso, s.torso);
    place(world, rig.head, s.head);
    for i in [L, R] {
        place(world, rig.eyes[i], s.eyes[i]);
        place(world, rig.upper[i], s.upper[i]);
        place(world, rig.fore[i], s.fore[i]);
        place(world, rig.wrists[i], s.hands[i]);
        place(world, rig.thighs[i], s.thighs[i]);
        place(world, rig.shins[i], s.shins[i]);
        place(world, rig.feet[i], s.feet[i]);
        // The hand closes into a fist past halfway (with a margin, so it does not flicker).
        let grip = match rig.grips[i] {
            Grip::Open if s.fists[i] > 0.58 => Grip::Fist,
            Grip::Fist if s.fists[i] < 0.42 => Grip::Open,
            g => g,
        };
        if grip != rig.grips[i] {
            rig.grips[i] = grip;
            for (k, g) in GRIPS.iter().enumerate() {
                show(world, rig.hands[i][k], *g == grip);
            }
        }
    }
    if let Some(mut r) = world.get_mut::<Rig>(root) {
        r.grips = rig.grips;
    }
}
/// The giant glows as `heat` says: its embers and rim (the casing's and the crust's), its furnace (the maw and its
/// light), its eyes and its weak spot's patch. Each material is written only when what it shows has changed enough to
/// see, so a giant breathing slowly costs a few small writes a second.
pub fn heat(world: &mut World, root: Entity, kit: &Kit, heat: Heat) {
    let dull = heat.dull.clamp(0., 1.);
    let colour = Vec3::from(EMBER).lerp(Vec3::from(DULL), dull);
    let glow = (colour * heat.embers.max(0.) * EMBERS).extend(0.);
    let rim = (Vec3::from(RIM) * (1. - 0.6 * dull)).extend(RIM_POWER);
    let near = |a: Vec4, b: Vec4| (a - b).abs().max_element() < 0.01;
    for (handle, share) in [(&kit.casing, 1.), (&kit.crust, 0.6)] {
        let want = (glow * share, rim);
        let same = world.resource::<Assets<TitanMaterial>>().get(handle).is_some_and(|m| near(m.extension.glow, want.0) && near(m.extension.rim, want.1));
        if !same {
            if let Some(mut m) = world.resource_mut::<Assets<TitanMaterial>>().get_mut(handle) {
                (m.extension.glow, m.extension.rim) = want;
            }
        }
    }
    let mut lit = |handle: &Handle<StandardMaterial>, want: Vec3| {
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let same = mats.get(handle).is_some_and(|m| (Vec3::from_slice(&m.base_color.to_linear().to_f32_array()[..3]) - want).abs().max_element() < 0.01);
        if !same {
            if let Some(mut m) = mats.get_mut(handle) {
                m.base_color = Color::linear_rgb(want.x, want.y, want.z);
            }
        }
    };
    lit(&kit.ember, Vec3::splat(1.7 * heat.maw.max(0.)) * Vec3::new(1., 1. - 0.5 * dull, 1. - 0.7 * dull));
    lit(&kit.eye, Vec3::new(1.2, 1.15, 1.0) * heat.eyes.max(0.));
    lit(&kit.spot, Vec3::from(COLD) * heat.spot.max(0.));
    if let Some(glow) = world.get::<Rig>(root).map(|r| r.glow) {
        if let Some(mut light) = world.get_mut::<PointLight>(glow) {
            let want = MAW_LIGHT * heat.maw.max(0.);
            if (light.intensity - want).abs() > MAW_LIGHT * 0.02 {
                light.intensity = want;
            }
        }
    }
}
/// How bright the embers are at a heat of 1 (added to the lit colour: about a dim lamp's).
const EMBERS: f32 = 0.45;
/// The maw's light on (High and Medium) or off.
pub fn light(world: &mut World, root: Entity, on: bool) {
    if let Some(glow) = world.get::<Rig>(root).map(|r| r.glow) {
        show(world, glow, on);
    }
}
/// Shows the whole titan, or (its own player in first person) only what can be seen from inside its head.
pub fn own_view(world: &mut World, root: Entity, own: bool) {
    let Some(rig) = world.get::<Rig>(root).cloned() else {
        return;
    };
    if rig.own == own {
        return;
    }
    for e in rig.unseen() {
        show(world, e, !own);
    }
    if let Some(mut r) = world.get_mut::<Rig>(root) {
        r.own = own;
    }
}

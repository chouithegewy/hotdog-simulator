// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The syrup rifle's optic: what the right button puts to the eye.
//!
//! * **The optic** is part of the rifle (weapons.rs): a short, chunky housing on a rail, a thick black ring towards the
//!   eye round its glass, lugs on the ring, a mount, brass-capped turrets. It is on the rifle at the hip as in every
//!   hotdog's hands. Aimed (`arena::tick`, over its AIM_TIME), the rifle comes up until the eye is on the optic's axis
//!   [`eye_relief`] behind its eyepiece: near enough that the ring fills the middle of the view and the glass is
//!   [`LENS_SHARE`] of the view's height across, the receiver low under it. Meanwhile the game's camera zooms [`ZOOM`]
//!   times ([`fov`]) and the mouse turns the view as many times slower (`arena::tick`), so that aiming is no twitchier
//!   magnified than it is at the hip. The view model keeps its own field of view (viewmodel.rs), so the optic does not
//!   swell with the zoom.
//! * **The lens** ([`LensMaterial`], `assets/shaders/scope_lens.wgsl`) is a disc on the glass in the first-person pass.
//!   It is drawn with that pass's opaque things and writes its depth, and everything else of the rifle and the hands
//!   holding it is drawn after the opaque things (`fit` gives them alpha-masked copies of their materials, which Bevy
//!   draws in the pass's second phase): so the optic's insides, the barrel, the front sight and the hand under the
//!   handguard never show through the glass, and the world, which the game's camera drew before, does, magnified.
//!   Over it the lens blends its own look: dark glass seen from the hip, clearing as it comes to the eye, a teal tint
//!   that deepens towards its edge, a thin bright rim, darker at its edge, and a faint reflection that drifts as the
//!   view turns. It costs one small draw.
//! * **The reticle**, drawn by the lens as signed distances, so that its lines are a pixel or two wide and sharp at any
//!   resolution: a syrup drop whose point is the aim, a thin ring broken at its four points round it, wide enough to
//!   frame a hotdog at four metres, two short stadia either side of it and a post below. It is lit syrup-amber, and
//!   over an enemy (`sight`) within the rifle's reach it turns that hotdog's colour, its team's in Forts, its own in
//!   Free For All (tags.rs `colour`), and its ring closes (a colour can be near the amber: Frankie's orange is). A
//!   team mate, whom the rifle's rounds pass by, leaves it amber, as does any hotdog in the siege's wave, when rounds
//!   hurt only the cats.
//! * **The readout**, in amber at the lens's lower edge in seven-segment digits and marks: the range to whatever the
//!   reticle is on in metres with a tenth (dimmed beyond the rifle's reach, dashes when nothing is in range), the
//!   magazine's count, and the burst as three pips that empty as its rounds go and fill again when the next is ready.
//! * **Round the lens**, by quality level ([`path`]): on every level a veil ([`VeilMaterial`]) darkens the view outside
//!   the ring, softly, more towards the edges of the screen; on Lowest and Low that is all there is (one quad blended
//!   in the pass the view model already has, no pass of its own), and it is darker. On Medium and High the picture is
//!   also blurred outside the ring (blur.rs: two fullscreen passes, across and down, on the view camera before tone
//!   mapping, only while the optic is up), and the veil is lighter. The head-up display is drawn after either.
//! * **The target's glow**: while the optic is up, the enemy under the reticle (as above) glows its colour round its
//!   silhouette, fading in and out in a tenth of a second or so. It is a shell over the hotdog's sausage and buns
//!   ([`GlowMaterial`], `assets/shaders/scope_glow.wgsl`), pushed out along their normals and added to the picture
//!   brightest just round the silhouette, drawn in the world's pass with its depth test: whatever stands in front of
//!   the hotdog hides it. It is this screen's alone and changes nothing in the round.
//!
//! The ketchup sniper keeps its own scope (arena.rs, `ScopeMask`): the screen black round a plain crosshair at 18
//! degrees, the gun put away.
use crate::arena::{Game, Weapon};
use crate::player_model::{Gait, PlayerBody, Rig};
use crate::quality::Level;
use bevy::{
    asset::RenderAssetUsages,
    camera::{
        primitives::{Aabb, MeshAabb},
        visibility::RenderLayers,
    },
    light::NotShadowCaster,
    mesh::{Indices, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{AsBindGroup, BlendState, PrimitiveTopology, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    shader::ShaderRef,
};
use kitchen_sim::feast::{self, Feast};
use std::collections::HashMap;

mod blur;
pub use blur::ScopeBlur;

/// How many times the syrup rifle's optic magnifies, aimed: the game's camera's view is that many times narrower.
pub const ZOOM: f32 = 2.2;
/// How much of the view's height the optic's glass is across, aimed.
pub const LENS_SHARE: f32 = 0.5;
/// The syrup-amber the reticle and the readout are lit in (linear).
pub const AMBER: [f32; 3] = [1.0, 0.50, 0.07];
/// The radius round a hotdog's chest (feast.rs `chest`) that the reticle is on it within: its body, as the rounds
/// find it.
const BODY: f32 = 0.055;
/// Seconds the reticle takes to change colour, and the target's glow to come up and to go.
const RETINT: f32 = 0.08;
const GLOW_IN: f32 = 0.10;
const GLOW_OUT: f32 = 0.15;
/// How far the glow's shell stands off the hotdog: so much, and so much more a metre away from the eye.
const SHELL: (f32, f32) = (0.0018, 0.0026);
/// The darkness round the lens, just outside the ring and at the edges of the view, on the levels without the blur
/// and on those with it; and the blur's size at its fullest, a share of the picture's height, and the share of the
/// height over which it comes up outside the ring.
const VEIL_PLAIN: (f32, f32) = (0.62, 0.9);
const VEIL_BLURRED: (f32, f32) = (0.42, 0.72);
const BLUR_SIZE: f32 = 0.026;
const BLUR_FEATHER: f32 = 0.07;
/// The veil's half size, in the lens's radii: past the edges of the widest view, however the rifle kicks, and on its
/// way up from the hip (where its edges would otherwise show).
const VEIL_SIZE: f32 = 40.;

/// How the view outside the lens is made out of focus at a quality level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// Lowest and Low: the veil alone, darker; no pass of its own.
    Veil,
    /// Medium and High: blurred (blur.rs) under a lighter veil.
    Blur,
}
/// The path a quality level takes.
pub fn path(level: Level) -> Path {
    match level {
        Level::High | Level::Medium => Path::Blur,
        Level::Low | Level::Lowest => Path::Veil,
    }
}

/// The magnification at `aim` (eased: 0 at the hip, 1 at the eye).
pub fn zoom(aim: f32) -> f32 {
    1. + (ZOOM - 1.) * aim.clamp(0., 1.)
}
/// The game camera's field of view at `aim` with the player's own `base` (radians, up and down the screen): the
/// view's tangent shrunk by the zoom, so that the middle of the view is magnified `zoom` times.
pub fn fov(base: f32, aim: f32) -> f32 {
    2. * ((base * 0.5).tan() / zoom(aim)).atan()
}
/// How far in front of the eye the optic's eyepiece is, aimed: where the glass, seen through the ring's bore, is
/// LENS_SHARE of the view model's view (viewmodel.rs FOV) high.
pub fn eye_relief() -> f32 {
    crate::weapons::RIFLE_LENS / (LENS_SHARE * (crate::viewmodel::FOV * 0.5).tan())
}

/// What the reticle is on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sighting {
    /// The nearest hotdog it covers that the eye can see, within the rifle's reach.
    pub target: Option<u32>,
    /// Whether that hotdog is an enemy: one the rifle's rounds would hurt now.
    pub enemy: bool,
    /// Metres to what it is on: that hotdog, or else the first thing in the way (a block, a loose object, the counter);
    /// None with nothing within the map's reach.
    pub range: Option<f32>,
}
/// What the reticle is on, seen from `eye` along the unit `dir`: the hotdogs are `bodies` (each one's id and the middle
/// of its chest, `feast::chest`, this player's own left out), `enemy` says which of them the rifle's rounds would hurt,
/// and `reach` is how far the rifle's rounds fly.
pub fn sight(feast: &Feast, eye: Vec3, dir: Vec3, bodies: &[(u32, Vec3)], enemy: impl Fn(u32) -> bool, reach: f32) -> Sighting {
    // The first thing in the way: a block or a loose object (feast.rs `screen`), or the counter's top.
    let mut surface = feast.screen(eye, dir);
    if dir.y < -1e-4 && eye.y > feast::FLOOR {
        surface = surface.min((feast::FLOOR - eye.y) / dir.y);
    }
    let hotdog = bodies
        .iter()
        .filter_map(|(id, chest)| kitchen_sim::ray_sphere(eye, dir, *chest, BODY).map(|t| (*id, t)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, t)| *t < surface);
    let range = hotdog.map_or(surface, |(_, t)| t);
    let target = hotdog.filter(|(_, t)| *t <= reach).map(|(id, _)| id);
    Sighting { target, enemy: target.is_some_and(enemy), range: (range < kitchen_sim::map::RANGE as f32 - 0.01).then_some(range) }
}

/// Whether a hotdog of team `theirs` is an enemy of this player's (of team `mine`): rounds hurt hotdogs now
/// (`Game::hostile`), and in Forts it is not on this player's team, whose rounds pass their own by.
pub fn foe(hostile: bool, forts: bool, mine: Option<u8>, theirs: Option<u8>) -> bool {
    hostile && !(forts && mine.is_some() && mine == theirs)
}

/// Seven-segment codes: the digits are themselves; then a dash, and a blank.
pub const DASH: u8 = 10;
pub const BLANK: u8 = 11;
/// The range readout's three digits (tens, units, tenths), the tens blank under ten metres; dashes with nothing in
/// range.
pub fn range_digits(range: Option<f32>) -> [u8; 3] {
    match range {
        Some(metres) => {
            let tenths = (metres.max(0.) * 10.).round().min(999.) as u32;
            let tens = (tenths / 100) as u8;
            [if tens == 0 { BLANK } else { tens }, (tenths / 10 % 10) as u8, (tenths % 10) as u8]
        }
        None => [BLANK, DASH, DASH],
    }
}
/// The magazine's count in two digits, the tens blank under ten (99 at the most).
pub fn count_digits(rounds: i32) -> [u8; 2] {
    let n = rounds.clamp(0, 99) as u8;
    [if n < 10 { BLANK } else { n / 10 }, n % 10]
}

/// The syrup rifle optic's lens: the glass, the reticle and the readout (see the module's documentation).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, PartialEq)]
pub struct LensMaterial {
    /// The reticle's colour (linear), and how bright it is lit.
    #[uniform(0)]
    pub reticle: Vec4,
    /// How far up the rifle is (0 at the hip, 1 at the eye), the reflection's drift (lens radii, up and across), and 1
    /// on the levels that blur round the lens, where the reticle is lit brighter for the bloom.
    #[uniform(0)]
    pub state: Vec4,
    /// The range readout's three digits (`range_digits`) and how bright it is (dimmer beyond the rifle's reach).
    #[uniform(0)]
    pub range: Vec4,
    /// The magazine's count (`count_digits`), the burst's rounds ready and its size.
    #[uniform(0)]
    pub counts: Vec4,
    /// How far the reticle is on an enemy (0 to 1), when its ring closes.
    #[uniform(0)]
    pub mark: Vec4,
}
impl Default for LensMaterial {
    fn default() -> Self {
        Self { reticle: Vec4::new(AMBER[0], AMBER[1], AMBER[2], 1.), state: Vec4::ZERO, range: Vec4::new(BLANK as f32, DASH as f32, DASH as f32, 1.), counts: Vec4::new(BLANK as f32, 0., 3., 3.), mark: Vec4::ZERO }
    }
}
impl Material for LensMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/scope_lens.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(_pipeline: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _layout: &MeshVertexBufferLayoutRef, _key: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Drawn with the opaque things, and written to the depth so that what is behind the glass and drawn after them
        // is hidden; but blended over the world already in the picture, premultiplied.
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
        Ok(())
    }
}

/// The darkness round the lens (see the module's documentation): a quad in the glass's plane with a hole the glass's
/// size.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct VeilMaterial {
    /// How dark just outside the ring and at the edges of the view, the ring's radius (in the lens's radii), and how far
    /// the veil is up (0 to 1).
    #[uniform(0)]
    pub shade: Vec4,
    /// The quad's half size, in the lens's radii.
    #[uniform(0)]
    pub size: Vec4,
}
impl Material for VeilMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/scope_veil.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// The glow round the hotdog under the reticle (see the module's documentation).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct GlowMaterial {
    /// Its colour (linear) and how far it is up (0 to 1).
    #[uniform(0)]
    pub colour: Vec4,
    /// How far the shell stands off the surface (metres, and more a metre away from the eye) and how bright its rim
    /// is.
    #[uniform(0)]
    pub shape: Vec4,
}
impl Material for GlowMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/scope_glow.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/scope_glow.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((MaterialPlugin::<LensMaterial>::default(), MaterialPlugin::<VeilMaterial>::default(), MaterialPlugin::<GlowMaterial>::default(), blur::Plugin))
            .init_resource::<Optic>()
            .add_systems(Update, optic.after(crate::arena::tick));
    }
}

/// The rifle's first-person model has its lens and veil, and its parts their masked materials (`fit`).
#[derive(Component)]
struct Fitted {
    veil: Entity,
    lens_material: Handle<LensMaterial>,
    veil_material: Handle<VeilMaterial>,
}
/// A glow's shell over one part of a hotdog.
#[derive(Component)]
struct Shell;

/// One hotdog's glow: how far it is up, its material and its shells.
struct Glow {
    level: f32,
    material: Handle<GlowMaterial>,
    shells: Vec<Entity>,
}

/// The optic as this frame has it.
#[derive(Resource, Default)]
pub struct Optic {
    /// How far the rifle is up, eased (0 at the hip, 1 at the eye); 0 with any other gun in hand.
    pub up: f32,
    /// What the reticle is on (nothing while the rifle is not up).
    pub sighting: Sighting,
    /// The reticle's colour now (linear), on its way to the one it is turning to, and how far it is on an enemy (0 to
    /// 1: its ring closes).
    pub reticle: [f32; 3],
    pub lock: f32,
    /// The quality level's path round the lens.
    pub path: Option<Path>,
    /// The glows up or going, by hotdog.
    glows: HashMap<u32, Glow>,
    /// The reflection's drift, and the view's yaw and pitch last frame.
    drift: Vec2,
    last: Option<(f32, f32)>,
    /// The alpha-masked copies of the rifle's and the hands' materials (`fit`), by the material they copy.
    masked: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}
impl Optic {
    /// How far the glow round hotdog `id` is up (0: none).
    #[cfg(test)]
    pub fn glow(&self, id: u32) -> f32 {
        self.glows.get(&id).map_or(0., |g| g.level)
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// A colour between `c` and white, `u` of the way, and never darker than a reticle can be seen in.
fn lit(c: [f32; 3], u: f32) -> [f32; 3] {
    let c = c.map(|v| v + (1. - v) * u);
    let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    if luma < 0.3 { c.map(|v| v + (0.3 - luma)) } else { c }
}
/// The sRGB colour `c` as linear.
fn linear(c: [f32; 3]) -> [f32; 3] {
    let l = Color::srgb(c[0], c[1], c[2]).to_linear();
    [l.red, l.green, l.blue]
}

/// A disc of `radius` facing +z (towards the eye in the gun frame), its texture coordinates 0 to 1 across it.
fn disc(radius: f32, sides: u32) -> Mesh {
    let mut positions = vec![[0., 0., 0.]];
    let mut uvs = vec![[0.5, 0.5]];
    for i in 0..=sides {
        let (s, c) = (i as f32 / sides as f32 * std::f32::consts::TAU).sin_cos();
        positions.push([c * radius, s * radius, 0.]);
        uvs.push([0.5 + c * 0.5, 0.5 - s * 0.5]);
    }
    let indices: Vec<u32> = (1..=sides).flat_map(|i| [0, i, i + 1]).collect();
    let normals = vec![[0., 0., 1.]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}
/// A square of half side `half` facing +z, its texture coordinates 0 to 1 across it.
fn square(half: f32) -> Mesh {
    let positions = vec![[-half, -half, 0.], [half, -half, 0.], [half, half, 0.], [-half, half, 0.]];
    let uvs = vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// Where the lens and the veil sit on the rifle's first-person model, under its root: on the glass, and just behind it.
pub fn lens_place() -> Transform {
    crate::weapons::in_view(Weapon::Rifle as usize) * Transform::from_translation(crate::weapons::RIFLE_SIGHT + Vec3::NEG_Z * (crate::weapons::RIFLE_GLASS - 0.00012))
}
fn veil_place() -> Transform {
    crate::weapons::in_view(Weapon::Rifle as usize) * Transform::from_translation(crate::weapons::RIFLE_SIGHT + Vec3::NEG_Z * (crate::weapons::RIFLE_GLASS + 0.0002))
}

/// Gives the rifle's first-person model (`root`) its lens and its veil, and the rifle's parts and the hands holding it
/// alpha-masked copies of their materials, so that they are drawn after the lens and never through it. Nothing
/// without the materials' assets (a test's world).
fn fit(world: &mut World, optic: &mut Optic, root: Entity) {
    if !world.contains_resource::<Assets<LensMaterial>>() || !world.contains_resource::<Assets<VeilMaterial>>() {
        return;
    }
    // Everything under the root that wears a material of its own.
    let mut below = vec![root];
    let mut worn = vec![];
    while let Some(e) = below.pop() {
        if let Some(children) = world.get::<Children>(e) {
            below.extend(children.to_vec());
        }
        if let Some(m) = world.get::<MeshMaterial3d<StandardMaterial>>(e) {
            worn.push((e, m.0.clone()));
        }
    }
    {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        for (_, handle) in &mut worn {
            let id = handle.id();
            if let Some(copy) = optic.masked.get(&id) {
                *handle = copy.clone();
                continue;
            }
            let Some(original) = materials.get(id).cloned() else { continue };
            if original.alpha_mode != AlphaMode::Opaque {
                continue;
            }
            let copy = materials.add(StandardMaterial { alpha_mode: AlphaMode::Mask(0.5), ..original });
            optic.masked.insert(id, copy.clone());
            *handle = copy;
        }
    }
    for (entity, handle) in worn {
        world.entity_mut(entity).insert(MeshMaterial3d(handle));
    }
    let (lens_mesh, veil_mesh) = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        (meshes.add(disc(crate::weapons::RIFLE_LENS, 48)), meshes.add(square(crate::weapons::RIFLE_LENS * VEIL_SIZE)))
    };
    let lens_material = world.resource_mut::<Assets<LensMaterial>>().add(LensMaterial::default());
    let veil_material = world.resource_mut::<Assets<VeilMaterial>>().add(VeilMaterial { shade: Vec4::ZERO, size: Vec4::new(VEIL_SIZE, 0., 0., 0.) });
    let layer = RenderLayers::layer(crate::viewmodel::VIEW_LAYER);
    let lens = world
        .spawn((Name::new("Syrup rifle optic lens"), Mesh3d(lens_mesh), MeshMaterial3d(lens_material.clone()), lens_place(), Visibility::Inherited, layer.clone(), NotShadowCaster))
        .id();
    let veil = world
        .spawn((Name::new("Syrup rifle optic veil"), Mesh3d(veil_mesh), MeshMaterial3d(veil_material.clone()), veil_place(), Visibility::Hidden, layer, NotShadowCaster))
        .id();
    world.entity_mut(root).add_children(&[lens, veil]).insert(Fitted { veil, lens_material, veil_material });
}

/// Every frame, after `arena::tick` has placed the camera and the rifle: fits the rifle's model, works out what the
/// reticle is on, and sets the lens, the veil, the blur and the glows by it (see the module's documentation).
pub fn optic(world: &mut World) {
    let Some(mut optic) = world.remove_resource::<Optic>() else { return };
    update(world, &mut optic);
    world.insert_resource(optic);
}

fn update(world: &mut World, optic: &mut Optic) {
    let Some(g) = world.get_resource::<Game>() else { return };
    let dt = world.get_resource::<Time<Real>>().map_or(1. / 60., |t| t.delta_secs()).clamp(0., 0.1);
    let level = world.get_resource::<crate::quality::Detail>().map_or(Level::High, |d| d.level);
    let route = path(level);
    let rifle = g.viewmodels().iter().find(|(w, _)| *w == Weapon::Rifle).map(|(_, e)| *e);
    let dead = g.roster.iter().any(|p| p.id == g.me() && !p.alive);
    let up = if g.weapon == Weapon::Rifle && g.playing() && !g.demo && !dead { ease(g.aim) } else { 0. };
    let (yaw, pitch) = (g.yaw, g.pitch);
    // What the reticle is on, once the rifle is most of the way up.
    let eye = g.camera().and_then(|c| world.get::<Transform>(c)).map(|t| t.translation);
    let sighting = match eye.filter(|_| up > 0.5) {
        Some(eye) => {
            let forts = g.forts();
            let me = g.me();
            let mine = g.roster.iter().find(|p| p.id == me).map(|p| p.team);
            let hostile = g.hostile();
            let bodies: Vec<(u32, Vec3)> = g.roster.iter().filter(|p| p.id != me && p.alive && !p.consumed).map(|p| (p.id, feast::chest(p))).collect();
            let roster = &g.roster;
            let enemy = |id: u32| foe(hostile, forts, mine, roster.iter().find(|p| p.id == id).map(|p| p.team));
            sight(&g.feast, eye, g.direction, &bodies, enemy, g.feast.tune.range[feast::RIFLE])
        }
        None => Sighting::default(),
    };
    // The reticle turns the enemy's colour, or back to amber.
    let lobby = world.get_resource::<crate::lobby::Lobby>();
    let wanted = match sighting.target.filter(|_| sighting.enemy).and_then(|id| g.roster.iter().find(|p| p.id == id)) {
        Some(p) => linear(lit(crate::tags::colour(p, g.forts(), lobby.and_then(|l| l.colour(p.id))), 0.12)),
        None => AMBER,
    };
    let k = 1. - (-dt / RETINT).exp();
    let reticle: [f32; 3] = if optic.reticle == [0.; 3] { wanted } else { std::array::from_fn(|i| optic.reticle[i] + (wanted[i] - optic.reticle[i]) * k) };
    let locked = sighting.target.is_some() && sighting.enemy;
    let lock = optic.lock + ((locked as u8 as f32) - optic.lock) * k;
    // The reflection drifts against the way the view turns, and settles back.
    let (dyaw, dpitch) = optic.last.map_or((0., 0.), |(y, p)| (((yaw - y + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI), pitch - p));
    let push = if dt > 1e-4 { Vec2::new(dyaw, -dpitch) / dt * 0.02 } else { Vec2::ZERO };
    let drift = (optic.drift + (push.clamp_length_max(0.25) - optic.drift) * (1. - (-dt * 6.).exp())).clamp_length_max(0.25);
    let (rounds, (ready, burst)) = (g.ammo[Weapon::Rifle as usize], g.burst());
    let reach = g.feast.tune.range[feast::RIFLE];
    let glowing = sighting.target.filter(|_| sighting.enemy && up > 0.6);
    let glow_colour = glowing.and_then(|id| g.roster.iter().find(|p| p.id == id)).map(|p| linear(lit(crate::tags::colour(p, g.forts(), lobby.and_then(|l| l.colour(p.id))), 0.1)));
    optic.up = up;
    optic.sighting = sighting;
    optic.reticle = reticle;
    optic.lock = lock;
    optic.path = Some(route);
    optic.drift = drift;
    optic.last = Some((yaw, pitch));
    // The rifle's model: fitted once, then its lens and veil set.
    if let Some(root) = rifle {
        if world.get::<Fitted>(root).is_none() {
            fit(world, optic, root);
        }
        if let Some(fitted) = world.get::<Fitted>(root) {
            let (veil, lens_material, veil_material) = (fitted.veil, fitted.lens_material.clone(), fitted.veil_material.clone());
            let strong = route == Path::Blur;
            let [r0, r1, r2] = range_digits(sighting.range);
            let [c0, c1] = count_digits(rounds);
            let look = LensMaterial {
                reticle: Vec4::new(reticle[0], reticle[1], reticle[2], if strong { 1.6 } else { 1. }),
                state: Vec4::new(up, drift.x, drift.y, strong as u8 as f32),
                range: Vec4::new(r0 as f32, r1 as f32, r2 as f32, if sighting.range.is_some_and(|r| r <= reach) { 1. } else { 0.45 }),
                counts: Vec4::new(c0 as f32, c1 as f32, ready as f32, burst as f32),
                mark: Vec4::new(lock, 0., 0., 0.),
            };
            // (Written only when it changes: a material written is prepared again.)
            if let Some(mut lenses) = world.get_resource_mut::<Assets<LensMaterial>>() {
                if lenses.get(&lens_material).is_some_and(|m| *m != look) {
                    if let Some(mut m) = lenses.get_mut(&lens_material) {
                        *m = look;
                    }
                }
            }
            let (near, far) = if strong { VEIL_BLURRED } else { VEIL_PLAIN };
            let fade = ease((up - 0.35) / 0.6);
            let shade = Vec4::new(near, far, crate::weapons::RIFLE_RING / crate::weapons::RIFLE_LENS, fade);
            if let Some(mut veils) = world.get_resource_mut::<Assets<VeilMaterial>>() {
                if veils.get(&veil_material).is_some_and(|m| m.shade != shade) {
                    if let Some(mut m) = veils.get_mut(&veil_material) {
                        m.shade = shade;
                    }
                }
            }
            if let Some(mut vis) = world.get_mut::<Visibility>(veil) {
                let want = if fade > 0.002 { Visibility::Inherited } else { Visibility::Hidden };
                if *vis != want {
                    *vis = want;
                }
            }
        }
    }
    blur_view(world, route, up, rifle);
    glow(world, optic, glowing, glow_colour, dt);
}

/// Puts the blur on the view camera while the optic is up on a level that blurs (`path`), round where the ring is on
/// the screen, and takes it off otherwise.
fn blur_view(world: &mut World, route: Path, up: f32, rifle: Option<Entity>) {
    let Some(view) = world.query_filtered::<Entity, With<crate::viewmodel::ViewCamera>>().iter(world).next() else { return };
    // (Once the ring is about the middle of the view: on the way up the middle stays sharp.)
    let fade = ease((up - 0.6) / 0.35);
    let placed = rifle.filter(|_| route == Path::Blur && fade > 0.01).and_then(|root| {
        let root = *world.get::<Transform>(root)?;
        // The eyepiece's face in the camera's frame, and the view's shape.
        let face = root.transform_point(crate::weapons::in_view(Weapon::Rifle as usize).transform_point(crate::weapons::RIFLE_SIGHT));
        let window = world.query::<&Window>().iter(world).next().map(|w| (w.width(), w.height()));
        let aspect = window.filter(|(w, h)| *w > 0. && *h > 0.).map_or(16. / 9., |(w, h)| w / h);
        let half = (crate::viewmodel::FOV * 0.5).tan();
        let depth = (-face.z).max(1e-4);
        let centre = Vec2::new(0.5 + face.x / depth / (half * aspect) * 0.5, 0.5 - face.y / depth / half * 0.5);
        let radius = crate::weapons::RIFLE_RING / depth / half * 0.5;
        Some(ScopeBlur { centre, radius, feather: BLUR_FEATHER, size: BLUR_SIZE * fade })
    });
    match placed {
        Some(blur) => {
            if world.get::<ScopeBlur>(view) != Some(&blur) {
                world.entity_mut(view).insert(blur);
            }
        }
        None => {
            if world.get::<ScopeBlur>(view).is_some() {
                world.entity_mut(view).remove::<ScopeBlur>();
            }
        }
    }
}

/// The glow round the enemy under the reticle (`glowing`, in `colour`), coming up, and every other one going.
fn glow(world: &mut World, optic: &mut Optic, glowing: Option<u32>, colour: Option<[f32; 3]>, dt: f32) {
    if !world.contains_resource::<Assets<GlowMaterial>>() {
        return;
    }
    if let Some(id) = glowing {
        if !optic.glows.contains_key(&id) {
            let material = world.resource_mut::<Assets<GlowMaterial>>().add(GlowMaterial { colour: Vec4::ZERO, shape: Vec4::new(SHELL.0, SHELL.1, 2.6, 0.) });
            optic.glows.insert(id, Glow { level: 0., material, shells: vec![] });
        }
    }
    let ids: Vec<u32> = optic.glows.keys().copied().collect();
    for id in ids {
        let Some(mut glow) = optic.glows.remove(&id) else { continue };
        let on = glowing == Some(id);
        glow.level = if on { (glow.level + dt / GLOW_IN).min(1.) } else { (glow.level - dt / GLOW_OUT).max(0.) };
        // Its shells, over the hotdog's body as it is drawn now (made again if the body was), nodding as it does.
        glow.shells.retain(|e| world.get_entity(*e).is_ok());
        if glow.shells.is_empty() && glow.level > 0. {
            glow.shells = shells(world, id, &glow.material);
        }
        nod(world, &glow.shells);
        if let Some(mut materials) = world.get_resource_mut::<Assets<GlowMaterial>>() {
            let was = materials.get(&glow.material).map(|m| m.colour);
            let wanted = match (on, colour, was) {
                (true, Some(c), _) => Some(Vec4::new(c[0], c[1], c[2], ease(glow.level))),
                (_, _, Some(was)) => Some(was.with_w(ease(glow.level))),
                _ => None,
            };
            if let Some(wanted) = wanted.filter(|w| Some(*w) != was) {
                if let Some(mut m) = materials.get_mut(&glow.material) {
                    m.colour = wanted;
                }
            }
        }
        if glow.level <= 0. && !on {
            for shell in glow.shells.drain(..) {
                if let Ok(e) = world.get_entity_mut(shell) {
                    e.despawn();
                }
            }
            world.resource_mut::<Assets<GlowMaterial>>().remove(&glow.material);
            continue;
        }
        optic.glows.insert(id, glow);
    }
}

/// Each shell bent as the part it covers is (player_model/bend.rs: the nod in the part's tag).
fn nod(world: &mut World, shells: &[Entity]) {
    for shell in shells {
        let tag = world.get::<ChildOf>(*shell).and_then(|parent| world.get::<bevy::mesh::MeshTag>(parent.parent()).map(|t| t.0));
        if let Some(tag) = tag.filter(|tag| world.get::<bevy::mesh::MeshTag>(*shell).map(|t| t.0) != Some(*tag)) {
            world.entity_mut(*shell).insert(bevy::mesh::MeshTag(tag));
        }
    }
}
/// Shells over hotdog `id`'s sausage and buns (the three largest parts on its torso that are shown), wearing `material`.
fn shells(world: &mut World, id: u32, material: &Handle<GlowMaterial>) -> Vec<Entity> {
    let torso = world.query::<(&Gait, &Rig, &PlayerBody)>().iter(world).find(|(gait, _, _)| gait.id() == id).map(|(_, rig, _)| rig.joints().torso);
    let Some(torso) = torso else { return vec![] };
    let parts: Vec<Entity> = world.get::<Children>(torso).map(|c| c.to_vec()).unwrap_or_default();
    let mut sized: Vec<(f32, Entity, Handle<Mesh>)> = parts
        .into_iter()
        .filter(|e| world.get::<Shell>(*e).is_none() && world.get::<Visibility>(*e).is_none_or(|v| *v != Visibility::Hidden))
        // (The sausage and the buns wear their bent material, player_model/bend.rs: they are the parts that are `Unbent`.)
        .filter(|e| world.get::<MeshMaterial3d<StandardMaterial>>(*e).is_some() || world.get::<crate::player_model::bend::Unbent>(*e).is_some())
        .filter_map(|e| {
            let mesh = world.get::<Mesh3d>(e)?.0.clone();
            // (Its bounds as the renderer has them, or else as its mesh has them, as in a test's world.)
            // (A bent part's bounds have room for its nod: its mesh's own are its size.)
            let bent = world.get::<crate::player_model::bend::Unbent>(e).is_some();
            let bounds = world.get::<Aabb>(e).copied().filter(|_| !bent).or_else(|| world.resource::<Assets<Mesh>>().get(&mesh)?.compute_aabb());
            let size = Vec3::from(bounds?.half_extents);
            let scale = world.get::<Transform>(e).map_or(Vec3::ONE, |t| t.scale);
            Some(((size * scale).abs().element_product(), e, mesh))
        })
        .collect();
    sized.sort_by(|a, b| b.0.total_cmp(&a.0));
    sized
        .into_iter()
        .take(3)
        .map(|(_, part, mesh)| {
            let shell = world.spawn((Name::new("Optic glow shell"), Shell, Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::IDENTITY, Visibility::Inherited, NotShadowCaster)).id();
            // A bent part's shell bends with it (its tag, `nod`), within the part's bounds.
            if let Some(room) = world.get::<bevy::camera::visibility::NoAutoAabb>(part).and(world.get::<Aabb>(part).copied()) {
                world.entity_mut(shell).insert((room, bevy::camera::visibility::NoAutoAabb));
            }
            world.entity_mut(part).add_child(shell);
            shell
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{test_app, test_frame, tick};
    use kitchen_sim::map::Map;

    /// A round played by `rules`, offline, with the optic's materials and its system after the round's, and a view
    /// camera for the blur to go on.
    fn app(rules: &str) -> App {
        let mut app = test_app(rules);
        app.init_asset::<LensMaterial>().init_asset::<VeilMaterial>().init_asset::<GlowMaterial>().init_resource::<Optic>();
        app.add_systems(Update, (tick, optic).chain());
        app.world_mut().spawn((crate::viewmodel::ViewCamera, Transform::default()));
        for _ in 0..3 {
            test_frame(&mut app, 0.05);
        }
        app
    }
    fn rifle(app: &App) -> Entity {
        app.world().resource::<Game>().viewmodels().iter().find(|(w, _)| *w == Weapon::Rifle).unwrap().1
    }
    fn right_button(app: &mut App, down: bool) {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        if down {
            mouse.press(MouseButton::Right);
        } else {
            mouse.release(MouseButton::Right);
        }
        test_frame(app, 0.01);
    }
    /// The rifle all the way up, the button held.
    fn raised(app: &mut App) {
        right_button(app, true);
        for _ in 0..5 {
            test_frame(app, 0.04);
        }
    }
    fn lens(app: &App) -> LensMaterial {
        let fitted = app.world().get::<Fitted>(rifle(app)).expect("the rifle is fitted");
        app.world().resource::<Assets<LensMaterial>>().get(&fitted.lens_material).unwrap().clone()
    }
    fn veil(app: &App) -> (VeilMaterial, bool) {
        let fitted = app.world().get::<Fitted>(rifle(app)).expect("the rifle is fitted");
        let shown = app.world().get::<Visibility>(fitted.veil) != Some(&Visibility::Hidden);
        (app.world().resource::<Assets<VeilMaterial>>().get(&fitted.veil_material).unwrap().clone(), shown)
    }
    /// A line of sight over the map's counter from a standing hotdog's eye, level, along which nothing is in the way for
    /// `clear` metres and something is before `within` metres (the map's reach, RANGE, for anything).
    fn line(feast: &Feast, clear: f32, within: f32) -> (Vec3, Vec3) {
        for i in 0..40 {
            for j in 0..30 {
                let (x, z) = (-3. + i as f32 * 0.15, -0.5 + j as f32 * 0.15);
                if !feast.level.clear(x as f64, z as f64) {
                    continue;
                }
                let eye = Vec3::new(x, feast.level.ground(x as f64, z as f64) as f32 + feast::EYE, z);
                for k in 0..16 {
                    let turn = k as f32 / 16. * std::f32::consts::TAU;
                    let dir = Vec3::new(-turn.sin(), 0., -turn.cos());
                    let far = feast.screen(eye, dir);
                    if far >= clear && far < within {
                        return (eye, dir);
                    }
                }
            }
        }
        panic!("no line {clear} m clear with something before {within} m on the map");
    }
    /// Puts the eye at `eye` looking along `dir`, with the rifle in hand all the way up, as `arena::tick` would have
    /// it, and hotdogs `others` with their chests where given (the rest out of the way, far behind).
    fn stage(app: &mut App, eye: Vec3, dir: Vec3, others: &[(u32, Vec3)]) {
        let camera = app.world().resource::<Game>().camera().unwrap();
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = Transform::from_translation(eye).looking_to(dir, Vec3::Y);
        let mut g = app.world_mut().resource_mut::<Game>();
        (g.direction, g.aim, g.weapon) = (dir, 1., Weapon::Rifle);
        for p in g.roster.iter_mut().filter(|p| p.id != 1) {
            let chest = others.iter().find(|(id, _)| *id == p.id).map_or(eye - dir * 30., |(_, at)| *at);
            (p.p, p.alive, p.prone, p.crouch) = ((chest - Vec3::Y * feast::CHEST[0]).to_array(), true, false, false);
        }
    }
    /// The optic's system run `n` times over the staged scene, without the round moving anyone.
    fn look(app: &mut App, n: usize) {
        for _ in 0..n {
            optic(app.world_mut());
        }
    }
    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 0.01)
    }

    /// Held, the right button brings the optic up with the rifle and lets it down again; the rifle's model is fitted
    /// with its lens and veil, and every part of it and the hands holding it are drawn after the lens (masked, or
    /// blended), so that nothing of the gun shows through the glass.
    #[test]
    fn the_optic_comes_up_with_the_right_button_and_nothing_of_the_rifle_is_drawn_through_its_lens() {
        let mut app = app("");
        assert_eq!(app.world().resource::<Optic>().up, 0.);
        let root = rifle(&app);
        let mut below = vec![root];
        let (mut parts, mut lenses) = (0, 0);
        while let Some(e) = below.pop() {
            below.extend(app.world().get::<Children>(e).map(|c| c.to_vec()).unwrap_or_default());
            if let Some(m) = app.world().get::<MeshMaterial3d<StandardMaterial>>(e) {
                let mode = app.world().resource::<Assets<StandardMaterial>>().get(&m.0).unwrap().alpha_mode;
                assert!(matches!(mode, AlphaMode::Mask(_) | AlphaMode::Blend), "{:?} is drawn with the opaque things: {mode:?}", app.world().get::<Name>(e));
                parts += 1;
            }
            lenses += app.world().get::<MeshMaterial3d<LensMaterial>>(e).is_some() as usize;
        }
        assert!(parts > 10 && lenses == 1, "{parts} parts, {lenses} lenses");
        assert!(!veil(&app).1 && lens(&app).state.x == 0., "at the hip: dark glass, no veil");
        raised(&mut app);
        assert_eq!(app.world().resource::<Optic>().up, 1.);
        assert!(veil(&app).1 && veil(&app).0.shade.w == 1. && lens(&app).state.x == 1., "up: the veil round the lens, the glass clear");
        right_button(&mut app, false);
        for _ in 0..5 {
            test_frame(&mut app, 0.04);
        }
        assert_eq!(app.world().resource::<Optic>().up, 0.);
        assert!(!veil(&app).1 && lens(&app).state.x == 0., "down again");
    }

    /// Lowest and Low go without the blur (the veil alone, darker; nothing on the view camera), Medium and High blur
    /// round the ring under a lighter veil, and the reticle is lit brighter there for the bloom.
    #[test]
    fn each_quality_level_takes_its_own_path_round_the_lens() {
        assert_eq!(Level::ALL.map(path), [Path::Blur, Path::Blur, Path::Veil, Path::Veil]);
        for level in Level::ALL {
            let mut app = app("");
            app.world_mut().insert_resource(crate::quality::Detail::of(level, crate::visuals::Quality::Full));
            let view = app.world_mut().query_filtered::<Entity, With<crate::viewmodel::ViewCamera>>().single(app.world()).unwrap();
            assert!(app.world().get::<ScopeBlur>(view).is_none());
            raised(&mut app);
            let blur = app.world().get::<ScopeBlur>(view).copied();
            let (shade, strong) = (veil(&app).0.shade, lens(&app).state.w);
            match path(level) {
                Path::Blur => {
                    let blur = blur.unwrap_or_else(|| panic!("{level:?}: no blur"));
                    assert!(blur.size > 0. && (blur.centre - Vec2::splat(0.5)).length() < 0.01, "{level:?}: round the middle of the view: {blur:?}");
                    assert!(blur.radius > 0.3 && blur.radius < 0.45, "{level:?}: sharp within the ring: {blur:?}");
                    assert_eq!((shade.x, shade.y, strong), (VEIL_BLURRED.0, VEIL_BLURRED.1, 1.), "{level:?}");
                }
                Path::Veil => {
                    assert!(blur.is_none(), "{level:?}: no pass of its own");
                    assert_eq!((shade.x, shade.y, strong), (VEIL_PLAIN.0, VEIL_PLAIN.1, 0.), "{level:?}");
                }
            }
            // Down, the blur goes.
            right_button(&mut app, false);
            for _ in 0..5 {
                test_frame(&mut app, 0.04);
            }
            assert!(app.world().get::<ScopeBlur>(view).is_none(), "{level:?}");
        }
    }

    /// In Free For All everyone is an enemy: over one within the rifle's reach the reticle turns its colour, and it
    /// glows; off it, the reticle is amber again and the glow goes. Only the hotdog under the reticle glows.
    #[test]
    fn over_an_enemy_the_reticle_turns_its_colour_and_it_alone_glows() {
        let mut app = app("sandbox mode=ffa botmode=idle god");
        raised(&mut app);
        assert!(app.world().resource::<Game>().hostile(), "Free For All: rounds hurt hotdogs");
        let (eye, dir) = line(&app.world().resource::<Game>().feast, 3.6, f32::INFINITY);
        let aside = dir.cross(Vec3::Y).normalize();
        stage(&mut app, eye, dir, &[(2, eye + dir * 2.), (3, eye + dir * 3. + aside * 0.4)]);
        look(&mut app, 12);
        let o = app.world().resource::<Optic>();
        let two = app.world().resource::<Game>().roster.iter().find(|p| p.id == 2).unwrap().clone();
        assert_eq!((o.sighting.target, o.sighting.enemy), (Some(2), true));
        assert!(near(o.reticle, linear(lit(crate::tags::colour(&two, false, None), 0.12))), "its colour: {:?}", o.reticle);
        assert!(o.lock > 0.99 && lens(&app).mark.x > 0.99, "its ring closed");
        assert!(!near(o.reticle, AMBER));
        assert_eq!((o.glow(2), o.glow(3)), (1., 0.), "it alone glows");
        let shells: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<Shell>>().iter(app.world()).collect();
        assert_eq!(shells.len(), 3, "over its sausage and its buns");
        let body = app.world_mut().query::<(Entity, &Gait)>().iter(app.world()).find(|(_, gait)| gait.id() == 2).unwrap().0;
        for shell in &shells {
            let mut up = *shell;
            while let Some(parent) = app.world().get::<ChildOf>(up) {
                up = parent.parent();
            }
            assert_eq!(up, body, "on hotdog 2");
        }
        // The reticle off it: amber again, and the glow gone, its shells with it.
        stage(&mut app, eye, dir, &[(2, eye + dir * 2. + aside * 0.3), (3, eye + dir * 3. + aside * 0.4)]);
        look(&mut app, 12);
        let o = app.world().resource::<Optic>();
        assert_eq!(o.sighting.target, None);
        assert!(near(o.reticle, AMBER) && o.glow(2) == 0.);
        assert_eq!(app.world_mut().query_filtered::<Entity, With<Shell>>().iter(app.world()).count(), 0);
        // Beyond the rifle's reach it is no target: amber, no glow.
        let reach = app.world().resource::<Game>().feast.tune.range[feast::RIFLE];
        let (eye, dir) = line(&app.world().resource::<Game>().feast, reach + 0.6, f32::INFINITY);
        stage(&mut app, eye, dir, &[(2, eye + dir * (reach + 0.3))]);
        look(&mut app, 12);
        let o = app.world().resource::<Optic>();
        assert!(o.sighting.target.is_none() && near(o.reticle, AMBER) && o.glow(2) == 0.);
        assert!(lens(&app).range.w < 1., "the range dimmed beyond the rifle's reach");
    }

    /// Behind cover a hotdog is not the target: the reticle stays amber, nothing glows, and the range is the cover's.
    #[test]
    fn a_hotdog_behind_cover_is_not_the_target_and_does_not_glow() {
        let mut app = app("sandbox mode=ffa botmode=idle god");
        raised(&mut app);
        let (eye, dir, wall) = {
            let feast = &app.world().resource::<Game>().feast;
            let (eye, dir) = line(feast, 0.9, 2.6);
            (eye, dir, feast.screen(eye, dir))
        };
        stage(&mut app, eye, dir, &[(2, eye + dir * (wall + 0.5))]);
        look(&mut app, 12);
        let o = app.world().resource::<Optic>();
        assert_eq!(o.sighting, Sighting { target: None, enemy: false, range: Some(wall) });
        assert!(near(o.reticle, AMBER) && o.glow(2) == 0.);
        // In front of the cover it is.
        stage(&mut app, eye, dir, &[(2, eye + dir * (wall - 0.3))]);
        look(&mut app, 12);
        let o = app.world().resource::<Optic>();
        assert_eq!((o.sighting.target, o.sighting.enemy), (Some(2), true));
        assert_eq!(o.glow(2), 1.);
    }

    /// Outside Free For All and the Forts fight the rounds pass hotdogs by: none is an enemy, the reticle stays
    /// amber. In Forts a team mate is never one (the rounds of a team pass its own by: there is no friendly fire), a
    /// rival is while the fight is on.
    #[test]
    fn a_team_mates_back_leaves_the_reticle_amber_and_so_does_anyone_while_rounds_pass_hotdogs_by() {
        let mut app = app("");
        raised(&mut app);
        assert!(!app.world().resource::<Game>().hostile(), "the siege: the rounds are the cats'");
        let (eye, dir) = line(&app.world().resource::<Game>().feast, 3., f32::INFINITY);
        stage(&mut app, eye, dir, &[(2, eye + dir * 2.)]);
        look(&mut app, 12);
        let o = app.world().resource::<Optic>();
        assert_eq!((o.sighting.target, o.sighting.enemy), (Some(2), false));
        assert!(near(o.reticle, AMBER) && o.glow(2) == 0.);
        // Forts: a team mate is no foe, fight or not; a rival is, in the fight.
        assert!(!foe(true, true, Some(0), Some(0)) && foe(true, true, Some(0), Some(1)) && !foe(false, true, Some(0), Some(1)));
        assert!(foe(true, false, Some(0), Some(0)), "Free For All: no teams");
        // A Forts round once its build phase is over is a fight, in which rounds hurt hotdogs.
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/network/fixtures/long-fort.json")).unwrap();
        app.add_plugins(crate::forts::Plugin);
        let mut g = app.world_mut().resource_mut::<Game>();
        g.feast = Feast::with_map(Map::parse(&text).unwrap(), false);
        g.begin();
        test_frame(&mut app, 0.05);
        app.world_mut().resource_mut::<Game>().feast.set_build(0.);
        for _ in 0..10 {
            test_frame(&mut app, 0.05);
        }
        let g = app.world().resource::<Game>();
        assert!(g.forts() && g.hostile(), "{}", g.phase_name());
    }

    /// The range readout is the simulation's own distance to what the reticle is on: a hotdog's body as the rounds
    /// find it, else the first thing in the way (`Feast::screen`) or the counter; tens blank under ten metres, dashes
    /// with nothing within the map's reach.
    #[test]
    fn the_range_is_the_simulations_distance_to_what_the_reticle_is_on() {
        let feast = Feast::with_map(Map::bundled(), false);
        let (eye, dir) = line(&feast, 2.6, 11.);
        let chest = eye + dir * 2. + Vec3::Y * 0.01;
        let s = sight(&feast, eye, dir, &[(2, chest)], |_| true, 4.);
        let body = kitchen_sim::ray_sphere(eye, dir, chest, 0.055).unwrap();
        assert_eq!(s, Sighting { target: Some(2), enemy: true, range: Some(body) });
        assert_eq!(range_digits(s.range), [BLANK, 1, 9], "{body}");
        // Nothing there: the wall.
        let wall = feast.screen(eye, dir);
        assert_eq!(sight(&feast, eye, dir, &[], |_| true, 4.).range, Some(wall));
        // Beyond the reach given: read, but no target.
        let s = sight(&feast, eye, dir, &[(2, chest)], |_| true, 1.5);
        assert_eq!((s.target, s.range), (None, Some(body)));
        // Looking down at the counter.
        let down = (dir + Vec3::NEG_Y * 0.4).normalize();
        let counter = (feast::FLOOR - eye.y) / down.y;
        assert!((sight(&feast, eye, down, &[], |_| true, 4.).range.unwrap() - counter.min(feast.screen(eye, down))).abs() < 1e-5);
        // Up at nothing within the map's reach.
        assert_eq!(sight(&feast, eye + Vec3::Y * 20., Vec3::Y, &[], |_| true, 4.).range, None);
        assert_eq!(range_digits(Some(3.46)), [BLANK, 3, 5]);
        assert_eq!(range_digits(Some(12.34)), [1, 2, 3]);
        assert_eq!(range_digits(Some(0.04)), [BLANK, 0, 0]);
        assert_eq!(range_digits(None), [BLANK, DASH, DASH]);
        assert_eq!((count_digits(60), count_digits(7), count_digits(0), count_digits(250)), ([6, 0], [BLANK, 7], [BLANK, 0], [9, 9]));
    }

    /// The readout in the lens is what the round has: the range the reticle reads, the magazine's count, and the
    /// burst's pips, which empty as a burst fires and fill when the rifle may fire again.
    #[test]
    fn the_readout_shows_the_range_the_magazine_and_the_burst_emptying_and_filling() {
        let mut app = app("sandbox mode=ffa botmode=idle god");
        raised(&mut app);
        let (eye, dir) = line(&app.world().resource::<Game>().feast, 3., f32::INFINITY);
        stage(&mut app, eye, dir, &[(2, eye + dir * 2.5)]);
        look(&mut app, 2);
        let (range, rounds) = (app.world().resource::<Optic>().sighting.range, app.world().resource::<Game>().ammo[Weapon::Rifle as usize]);
        let digits = range_digits(range).map(|d| d as f32);
        let l = lens(&app);
        assert_eq!((l.range.x, l.range.y, l.range.z), (digits[0], digits[1], digits[2]));
        let count = count_digits(rounds).map(|d| d as f32);
        assert_eq!((l.counts.x, l.counts.y, l.counts.z, l.counts.w), (count[0], count[1], 3., 3.));
        // A burst: the trigger pulled once.
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        test_frame(&mut app, 0.01);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        let mut seen = vec![app.world().resource::<Game>().burst().0];
        for _ in 0..40 {
            test_frame(&mut app, 0.02);
            let ready = app.world().resource::<Game>().burst().0;
            if seen.last() != Some(&ready) {
                seen.push(ready);
            }
        }
        assert_eq!(seen, [2, 1, 0, 3], "the pips go as the rounds do, and fill again");
        assert_eq!(lens(&app).counts.z, 3.);
        assert_eq!(app.world().resource::<Game>().ammo[Weapon::Rifle as usize], rounds - 3);
    }
}

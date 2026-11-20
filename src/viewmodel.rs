// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The first-person arms and weapon, drawn the way Halo draws them: in a pass of their own over the
//! finished scene, so they can never sink into a wall the player is standing against.
//!
//! * **The pass.** Everything parented to the game's camera (`maya::HotdogCamera`: the weapon models,
//!   their hands, the stim forearm and its lightning) is moved to render layer [`VIEW_LAYER`], which
//!   the game's camera does not draw. A second camera, [`ViewCamera`], draws only that layer, after
//!   the world, into the same picture with the depth buffer cleared: whatever the world put there,
//!   the view model is in front of it. It keeps the field of view the models were built for, so it
//!   does not zoom with the scope.
//! * **One picture.** Both cameras share one high-dynamic-range picture, and the steps that finish
//!   it (bloom, tone mapping, colour grading, sharpening, the vignette) are handed from the game's
//!   camera to the view camera, which comes last. The world and the weapon are therefore bloomed,
//!   tone mapped and vignetted once, together; fog and the volume of lit air stay with the world.
//!   `visuals` still decides what those steps are: [`stack`] only moves what it finds.
//! * **Light.** The lamps light both layers. The sun does not, because a sun that reached the view
//!   camera would cast its shadow cascades a second time for it: instead a copy of the sun without
//!   shadows lights the view model alone, and is dimmed when a block stands between the player and
//!   the sun, so the weapon goes into shade when the player does.
//! * **The hands.** [`hands`] builds a hotdog's forearm and mitten around each weapon's grip, with
//!   a second hand under the long ones. They are children of the weapon's root, so they recoil,
//!   swap and dip with it.
use crate::maya::HotdogCamera;
use bevy::{
    anti_alias::{
        contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa, taa::TemporalAntiAliasing,
    },
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, CameraOutputMode, Exposure, Hdr},
    light::cluster::ClusterConfig,
    core_pipeline::tonemapping::Tonemapping,
    light::NotShadowCaster,
    mesh::Indices,
    post_process::{
        bloom::Bloom,
        effect_stack::{ChromaticAberration, Vignette},
    },
    prelude::*,
    render::{
        render_resource::{BlendState, PrimitiveTopology},
        view::ColorGrading,
    },
};
use std::f32::consts::{PI, TAU};

/// The render layer of the first-person view model. The world is on layer 0.
pub const VIEW_LAYER: usize = 1;
/// The view model's vertical field of view: the one `arena::tick` gives the game's camera when it
/// is not scoped, which the weapon models and the muzzle offset of the player's own rounds were
/// placed for.
pub const FOV: f32 = 1.483_529_9;
/// Nearer than any part of a hand or a weapon comes to the eye.
const NEAR: f32 = 0.004;

/// The camera that draws the view model over the world.
#[derive(Component)]
pub struct ViewCamera;
/// The root of one weapon's first-person model, and which weapon it is. `arena::tick` shows, moves
/// and kicks it; the hands hang from it.
#[derive(Component)]
pub struct ViewModel(#[allow(dead_code)] pub usize);
/// One first-person hand: its forearm and mitten in one mesh.
#[derive(Component)]
pub struct Hand;
/// The sun's stand-in for the view model.
#[derive(Component)]
struct ViewSun;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hands>()
            .add_systems(Update, (layer, light).after(crate::arena::tick))
            .add_systems(
                PostUpdate,
                (stack, follow, sun).before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Moves everything that hangs from the game's camera to the view model's layer, and stops it
/// casting shadows: a weapon floating in front of the eye has no body to cast them with. Runs after
/// `arena::tick` has built or rebuilt the view models, so nothing is ever drawn on the wrong layer.
fn layer(
    mut commands: Commands,
    cameras: Query<Entity, With<HotdogCamera>>,
    children: Query<&Children>,
    layered: Query<(), With<RenderLayers>>,
) {
    for camera in &cameras {
        for entity in children.iter_descendants(camera) {
            if !layered.contains(entity) {
                commands
                    .entity(entity)
                    .try_insert((RenderLayers::layer(VIEW_LAYER), NotShadowCaster));
            }
        }
    }
}

/// Lamps light the world and the view model alike. Their shadow maps belong to the lamp, not to a
/// camera, so this costs nothing.
fn light(
    mut commands: Commands,
    lamps: Query<Entity, (Or<(With<PointLight>, With<SpotLight>)>, Without<RenderLayers>)>,
) {
    for lamp in &lamps {
        commands.entity(lamp).try_insert(RenderLayers::from_layers(&[0, VIEW_LAYER]));
    }
}

/// Puts the view camera at the end of the game's camera: same picture, same exposure and fill
/// light, and the finishing steps moved onto it so they run once, over the world and the view
/// model together.
#[allow(clippy::type_complexity)]
fn stack(
    mut commands: Commands,
    mut games: Query<
        (Entity, &mut Camera, &mut Tonemapping, Has<Hdr>, &Msaa, Has<TemporalAntiAliasing>),
        (With<HotdogCamera>, Without<ViewCamera>),
    >,
    lenses: Query<
        (Option<&Exposure>, Option<Ref<ColorGrading>>, Option<Ref<AmbientLight>>, Option<Ref<EnvironmentMapLight>>),
        (With<HotdogCamera>, Without<ViewCamera>),
    >,
    finishes: Query<
        (Option<&Bloom>, Option<&Vignette>, Option<&ChromaticAberration>, Option<&ContrastAdaptiveSharpening>),
        (With<HotdogCamera>, Without<ViewCamera>),
    >,
    mut views: Query<
        (
            Entity,
            &mut Camera,
            &mut Tonemapping,
            &mut Msaa,
            Has<Hdr>,
            Has<Fxaa>,
            Option<&Exposure>,
            Ref<ViewCamera>,
        ),
        With<ViewCamera>,
    >,
    mut method: Local<Option<Tonemapping>>,
    detail: Option<Res<crate::quality::Detail>>,
) {
    let Some((game, mut camera, mut tonemapping, hdr, msaa, temporal)) = games.iter_mut().next() else {
        return;
    };
    let Some((view, mut lens, mut mapping, mut samples, view_hdr, smoothed, exposed, marker)) = views.iter_mut().next()
    else {
        // Idle until the next frame has matched it to the game's camera.
        commands.spawn((
            Name::new("First-person view model camera"),
            ViewCamera,
            Camera3d::default(),
            Msaa::default(),
            Tonemapping::default(),
            Camera {
                order: 1,
                is_active: false,
                // The world is already in the picture.
                clear_color: ClearColorConfig::None,
                // It finishes the picture and puts it on screen, exactly as a lone camera would.
                output_mode: CameraOutputMode::Write {
                    blend_state: Some(BlendState::REPLACE),
                    clear_color: ClearColorConfig::Default,
                },
                ..default()
            },
            Projection::Perspective(PerspectiveProjection { fov: FOV, near: NEAR, ..default() }),
            RenderLayers::layer(VIEW_LAYER),
            Transform::default(),
            // What it draws is a hand and a gun a few centimetres from the eye, lit by a handful of lamps: one
            // cluster of lights for all of it, instead of sorting every lamp into thousands of them every frame.
            ClusterConfig::Single,
        ));
        return;
    };
    // The game's camera leaves the picture unfinished for the view camera, which is the one that
    // puts it on screen.
    if *tonemapping != Tonemapping::None {
        *method = Some(*tonemapping);
    }
    let wanted = method.unwrap_or_default();
    // Without HDR a camera tone maps in its own shaders, so each keeps the method.
    let (left, passed) = if hdr { (Tonemapping::None, wanted) } else { (wanted, wanted) };
    if *tonemapping != left {
        *tonemapping = left;
    }
    if *mapping != passed {
        *mapping = passed;
    }
    if !matches!(camera.output_mode, CameraOutputMode::Skip) {
        camera.output_mode = CameraOutputMode::Skip;
    }
    if lens.is_active != camera.is_active {
        lens.is_active = camera.is_active;
    }
    // One picture is shared only by cameras that agree on its format and its anti-aliasing.
    if *samples != *msaa {
        *samples = *msaa;
    }
    let mut view = commands.entity(view);
    match (hdr, view_hdr) {
        (true, false) => {
            view.insert(Hdr);
        }
        (false, true) => {
            view.remove::<Hdr>();
        }
        _ => {}
    }
    let Ok((exposure, grading, ambient, environment)) = lenses.get(game) else {
        return;
    };
    if let Some(exposure) = exposure {
        if exposed.is_none_or(|mine| mine.ev100 != exposure.ev100) {
            view.insert(*exposure);
        }
    }
    // The grade is applied where the picture is tone mapped, and the fill light where the view
    // model is lit; `visuals` changes both on the game's camera as it pleases, so they are simply
    // kept the same: copied when they change (or the view camera is new), not every frame.
    let fresh = marker.is_added();
    if let Some(grading) = grading.filter(|g| fresh || g.is_changed()) {
        view.insert((*grading).clone());
    }
    if let Some(ambient) = ambient.filter(|a| fresh || a.is_changed()) {
        view.insert((*ambient).clone());
    }
    if let Some(environment) = environment.filter(|e| fresh || e.is_changed()) {
        view.insert((*environment).clone());
    }
    // Temporal anti-aliasing belongs to the world's pass and never sees the view model, so where
    // the world uses it the finished picture gets a pass of FXAA for the weapon's edges; so does a
    // picture that is not multisampled at all (the lower levels of `quality` trade MSAA for FXAA).
    // Elsewhere both passes are multisampled into the shared picture.
    // (The level may do without FXAA where there is no multisampling, `quality::Detail::fxaa`.)
    let fxaa = temporal || (*msaa == Msaa::Off && detail.as_ref().is_none_or(|d| d.fxaa));
    match (fxaa, smoothed) {
        (true, false) => {
            view.insert(Fxaa::default());
        }
        (false, true) => {
            view.remove::<Fxaa>();
        }
        _ => {}
    }
    let Ok((bloom, vignette, aberration, sharpening)) = finishes.get(game) else {
        return;
    };
    if let Some(bloom) = bloom {
        view.insert(bloom.clone());
    }
    if let Some(vignette) = vignette {
        view.insert(vignette.clone());
    }
    if let Some(aberration) = aberration {
        view.insert(aberration.clone());
    }
    if let Some(sharpening) = sharpening {
        view.insert(sharpening.clone());
    }
    if bloom.is_some() || vignette.is_some() || aberration.is_some() || sharpening.is_some() {
        commands
            .entity(game)
            .remove::<(Bloom, Vignette, ChromaticAberration, ContrastAdaptiveSharpening)>();
    }
}

/// Keeps the view camera exactly where the game's camera is, whatever that hangs from, and at the
/// field of view the models were built for.
fn follow(
    games: Query<Entity, (With<HotdogCamera>, Without<ViewCamera>)>,
    parents: Query<&ChildOf>,
    transforms: Query<&Transform, Without<ViewCamera>>,
    mut views: Query<(&mut Transform, &mut Projection), With<ViewCamera>>,
) {
    let Some(game) = games.iter().next() else {
        return;
    };
    let Ok(mut place) = transforms.get(game).copied() else {
        return;
    };
    let mut entity = game;
    while let Ok(parent) = parents.get(entity) {
        entity = parent.parent();
        if let Ok(outer) = transforms.get(entity) {
            place = outer.mul_transform(place);
        }
    }
    for (mut transform, mut projection) in &mut views {
        if *transform != place {
            *transform = place;
        }
        if let Projection::Perspective(lens) = &mut *projection {
            if lens.fov != FOV || lens.near != NEAR {
                (lens.fov, lens.near) = (FOV, NEAR);
            }
        }
    }
}

/// Whether anything of the map stands between `from` and the sun, which lies along `to_sun`.
pub fn shaded(scene: &crate::ragdoll::Scene, from: Vec3, to_sun: Vec3) -> bool {
    scene.blocks.iter().any(|block| {
        // The slab test: where the ray is inside the box on all three axes at once.
        let (mut enter, mut leave) = (0.01f32, 10f32);
        for axis in 0..3 {
            let (origin, way) = (from[axis], to_sun[axis]);
            if way.abs() < 1e-6 {
                if origin < block.min[axis] || origin > block.max[axis] {
                    return false;
                }
                continue;
            }
            let (a, b) = ((block.min[axis] - origin) / way, (block.max[axis] - origin) / way);
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
        }
        enter <= leave
    })
}

/// Lights the view model with a copy of the brightest sun: same colour, same direction, no
/// shadow maps. Standing in the shadow of a block dims it, sampled around where the weapon is
/// held so the change is gradual at a shadow's edge.
fn sun(
    mut commands: Commands,
    time: Res<Time<Real>>,
    game: Option<Res<crate::arena::Game>>,
    deaths: Option<Res<crate::death::Deaths>>,
    suns: Query<(&DirectionalLight, &GlobalTransform, Option<&RenderLayers>), Without<ViewSun>>,
    mut copies: Query<(&mut DirectionalLight, &mut Transform), With<ViewSun>>,
    mut lit: Local<Option<f32>>,
) {
    let Some((mut copy, mut transform)) = copies.iter_mut().next() else {
        commands.spawn((
            Name::new("Sun on the first-person view model"),
            ViewSun,
            DirectionalLight { illuminance: 0., shadow_maps_enabled: false, ..default() },
            RenderLayers::layer(VIEW_LAYER),
            Transform::default(),
        ));
        return;
    };
    let brightest = suns
        .iter()
        .filter(|(_, _, layers)| layers.is_none_or(|layers| layers.intersects(&RenderLayers::layer(0))))
        .max_by(|a, b| a.0.illuminance.total_cmp(&b.0.illuminance));
    let Some((sun, place, _)) = brightest else {
        if copy.illuminance != 0. {
            copy.illuminance = 0.;
        }
        return;
    };
    let to_sun = place.back().as_vec3();
    let open = match (&game, &deaths) {
        (Some(game), Some(deaths)) if game.playing() => {
            let eye = game.position + Vec3::Y * 0.11;
            let (ahead, right) = (Quat::from_rotation_y(game.yaw) * Vec3::NEG_Z, Quat::from_rotation_y(game.yaw) * Vec3::X);
            let around = [Vec3::ZERO, ahead * 0.08, ahead * 0.04 + right * 0.05, ahead * 0.04 - right * 0.05, Vec3::Y * 0.03];
            around.iter().filter(|offset| !shaded(&deaths.scene, eye + **offset, to_sun)).count() as f32 / around.len() as f32
        }
        _ => 1.,
    };
    let now = lit.get_or_insert(open);
    *now += (open - *now) * (1. - (-time.delta_secs().min(0.1) * 9.).exp());
    let illuminance = sun.illuminance * *now;
    if copy.illuminance != illuminance {
        copy.illuminance = illuminance;
    }
    if copy.color != sun.color {
        copy.color = sun.color;
    }
    let turn = place.rotation();
    if transform.rotation != turn {
        transform.rotation = turn;
    }
}

/// Triangles with a normal and a texture coordinate per corner.
#[derive(Clone, Default)]
pub struct Shape {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
}
impl Shape {
    /// Joins a grid of corners laid down row by row from `base` into triangles that face the way
    /// their normals do.
    fn knit(&mut self, base: usize, rows: usize, columns: usize) {
        for row in 0..rows - 1 {
            for column in 0..columns - 1 {
                let a = (base + row * columns + column) as u32;
                let (b, c, d) = (a + 1, a + columns as u32, a + columns as u32 + 1);
                for [i, j, k] in [[a, c, b], [b, c, d]] {
                    let [p, q, r] = [i, j, k].map(|corner| self.positions[corner as usize]);
                    let face = (q - p).cross(r - p);
                    // A pole has no area.
                    if face.length_squared() < 1e-18 {
                        continue;
                    }
                    let out = [i, j, k].iter().map(|corner| self.normals[*corner as usize]).sum::<Vec3>();
                    self.indices.extend(if face.dot(out) >= 0. { [i, j, k] } else { [i, k, j] });
                }
            }
        }
    }
    /// A rounded lump: the heel of a hand.
    pub(crate) fn blob(&mut self, centre: Vec3, radii: Vec3, turn: Quat) {
        const RINGS: usize = 10;
        const SIDES: usize = 16;
        let base = self.positions.len();
        for ring in 0..=RINGS {
            let (across, up) = (ring as f32 / RINGS as f32 * PI).sin_cos();
            for side in 0..=SIDES {
                let (z, x) = (side as f32 / SIDES as f32 * TAU).sin_cos();
                let unit = Vec3::new(across * x, up, across * z);
                self.positions.push(centre + turn * (unit * radii));
                self.normals.push((turn * (unit / radii)).normalize_or(Vec3::Y));
                self.uvs.push(Vec2::new(side as f32 / SIDES as f32 * 0.4, ring as f32 / RINGS as f32 * 0.2));
            }
        }
        self.knit(base, RINGS + 1, SIDES + 1);
    }
    /// A finger, a thumb or a forearm: an oval swept along a smooth path through `points` and
    /// rounded off at both ends. Each point carries the oval's half-width across and its
    /// half-height along `tall`.
    pub(crate) fn tube(&mut self, points: &[(Vec3, f32, f32)], tall: Vec3) {
        const STEPS: usize = 4;
        const SIDES: usize = 12;
        const CAP: usize = 4;
        let count = points.len() as isize;
        let at = |i: isize| points[i.clamp(0, count - 1) as usize];
        // A Catmull-Rom spline through the points.
        let mut spine: Vec<(Vec3, f32, f32)> = vec![];
        for span in 0..count - 1 {
            let (p0, p1, p2, p3) = (at(span - 1), at(span), at(span + 1), at(span + 2));
            for step in 0..STEPS {
                let t = step as f32 / STEPS as f32;
                let place = 0.5
                    * (2. * p1.0
                        + (p2.0 - p0.0) * t
                        + (2. * p0.0 - 5. * p1.0 + 4. * p2.0 - p3.0) * t * t
                        + (3. * p1.0 - p0.0 - 3. * p2.0 + p3.0) * t * t * t);
                spine.push((place, p1.1 + (p2.1 - p1.1) * t, p1.2 + (p2.2 - p1.2) * t));
            }
        }
        spine.push(at(count - 1));
        let last = spine.len() - 1;
        let base = self.positions.len();
        let mut along = 0.;
        let mut rows = 0;
        // Each row: a place on the spine, how far round an end cap it is (0 on the tube itself),
        // and which way that cap bulges.
        let caps = |end: f32| (1..=CAP).map(move |k| (k as f32 / CAP as f32 * PI * 0.5, end));
        let start: Vec<(usize, f32, f32)> = caps(-1.).map(|(angle, end)| (0, angle, end)).collect();
        let body = (0..=last).map(|i| (i, 0., 0.));
        let finish = caps(1.).map(|(angle, end)| (last, angle, end));
        for (i, angle, end) in start.into_iter().rev().chain(body).chain(finish) {
            let (place, across, height) = spine[i];
            let tangent = (spine[(i + 1).min(last)].0 - spine[i.saturating_sub(1)].0).normalize_or(Vec3::Z);
            let up = (tall - tangent * tall.dot(tangent)).normalize_or(tangent.any_orthonormal_vector());
            let side = up.cross(tangent);
            if i > 0 && angle == 0. {
                along += place.distance(spine[i - 1].0);
            }
            let (bulge, shrink) = angle.sin_cos();
            let centre = place + tangent * end * bulge * across;
            for corner in 0..=SIDES {
                let (sin, cos) = (corner as f32 / SIDES as f32 * TAU).sin_cos();
                let rim = (side * cos / across + up * sin / height).normalize_or(up);
                self.positions.push(centre + (side * cos * across + up * sin * height) * shrink);
                self.normals.push((rim * shrink + tangent * end * bulge).normalize_or(rim));
                self.uvs.push(Vec2::new((along + end * bulge * across) / 0.16, corner as f32 / SIDES as f32 * 0.25));
            }
            rows += 1;
        }
        self.knit(base, rows, SIDES + 1);
    }
    /// The same shape for the other hand: reflected left to right.
    pub(crate) fn mirrored(mut self) -> Self {
        for v in self.positions.iter_mut().chain(self.normals.iter_mut()) {
            v.x = -v.x;
        }
        for triangle in self.indices.chunks_mut(3) {
            triangle.swap(1, 2);
        }
        self
    }
    pub(crate) fn mesh(&self) -> Mesh {
        let list = |v: &[Vec3]| v.iter().map(|p| p.to_array()).collect::<Vec<_>>();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, list(&self.positions))
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, list(&self.normals))
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.iter().map(|uv| uv.to_array()).collect::<Vec<_>>())
            .with_inserted_indices(Indices::U32(self.indices.clone()));
        // The sausage skin has a normal map, which wants tangents; without them it is only smoother.
        let _ = mesh.generate_tangents();
        mesh
    }
}

/// How thick a finger is, in metres in the view.
const FINGER: f32 = 0.0037;
/// The way each forearm leaves its hand, in the frame of the weapon's root: back towards the
/// player and out to the lower corner of the view on its own side.
const RIGHT_ARM: Vec3 = Vec3::new(0.74, -0.50, 0.45);
const LEFT_ARM: Vec3 = Vec3::new(-0.45, -0.72, 0.53);

/// What the first finger does while the others hold on.
#[derive(Clone, Copy)]
enum Index {
    /// Curls with the rest of the mitten.
    Curled,
    /// Rests on a trigger whose face is at this point.
    Trigger(Vec3),
    /// Reaches up and over to press a button whose top is at this point.
    Button(Vec3),
}

/// A right hand closed round an upright handle: a pistol grip, a foregrip, a can. `middle` is
/// the middle of the handle and `axis` runs up it; the handle is `wide` from side to side, `deep`
/// from back to front and `tall` (all halved). The mitten wraps the front, the thumb lies along
/// the far side, the heel of the hand is behind, and the forearm leaves along `arm`.
fn fist(middle: Vec3, axis: Vec3, (wide, deep, tall): (f32, f32, f32), index: Index, arm: Vec3, reach: f32) -> Shape {
    let up = axis.normalize();
    let back = Vec3::X.cross(up).normalize();
    let right = up.cross(back);
    // Handle space: x to the hand's own side, y up the handle, z back towards the wrist.
    let place = |x: f32, y: f32, z: f32| middle + right * x + up * y + back * z;
    let inside = |p: Vec3| Vec3::new((p - middle).dot(right), (p - middle).dot(up), (p - middle).dot(back));
    let f = FINGER;
    let mut shape = Shape::default();
    // The mitten: all four fingers if the first has nothing better to do, the lower three otherwise.
    let (half, level) = match index {
        Index::Curled => (tall.min(0.0088), 0.),
        _ => (tall * 0.6, -tall * 0.42),
    };
    let mut wrap = vec![
        (place(wide + f * 0.85, level, deep * 0.9), f, half),
        (place(wide + f * 0.95, level, -deep * 0.3), f, half),
        (place(wide * 0.75, level, -deep - f * 0.9), f, half),
        (place(-wide * 0.75, level, -deep - f * 0.9), f, half),
        (place(-wide - f * 0.9, level, -deep * 0.35), f, half),
        (place(-wide - f * 0.7, level, deep * 0.3), f * 0.9, half * 0.92),
    ];
    // A hand this small does not reach all the way round a can.
    if wide > 0.008 {
        wrap.truncate(4);
    }
    shape.tube(&wrap, up);
    let knuckle = match index {
        Index::Curled => tall * 0.45,
        _ => tall - 0.0032,
    };
    shape.tube(
        &[
            (place(wide * 0.15, knuckle - 0.0012, deep + f * 0.8), 0.0036, 0.0036),
            (place(-wide - f * 0.7, knuckle + 0.0002, deep * 0.35), 0.0034, 0.0034),
            (place(-wide - f * 0.85, knuckle + 0.0008, -deep * 0.3), 0.0031, 0.0031),
            (place(-wide - f * 0.7, knuckle + 0.0008, -deep * 0.85), 0.0027, 0.0027),
        ],
        up,
    );
    match index {
        Index::Curled => {}
        Index::Trigger(face) => {
            let face = inside(face);
            shape.tube(
                &[
                    (place(wide + f * 0.8, face.y + 0.001, deep * 0.8), 0.0033, 0.0033),
                    (place(wide + f * 0.95, face.y + 0.0005, -deep * 0.5), 0.0032, 0.0032),
                    (place(wide * 0.6 + f * 0.5, face.y, face.z - 0.0018), 0.0030, 0.0030),
                    (place(0.0004, face.y, face.z - 0.0028), 0.0027, 0.0027),
                ],
                up,
            );
        }
        Index::Button(top) => {
            let top = inside(top);
            shape.tube(
                &[
                    (place(wide * 0.88 + f * 0.7, tall * 0.3, deep * 0.47 + f * 0.35), 0.0034, 0.0034),
                    (place(wide * 0.88 + f * 0.7, top.y * 0.6, deep * 0.47 + f * 0.35), 0.0034, 0.0034),
                    (place(wide * 0.55 + f * 0.5, top.y * 0.8, deep * 0.3 + f * 0.25), 0.0033, 0.0033),
                    (place(wide * 0.25, top.y + f * 0.5, deep * 0.12), 0.0031, 0.0031),
                    (place(0., top.y + 0.0028, top.z), 0.0029, 0.0029),
                ],
                back,
            );
        }
    }
    let turn = Quat::from_mat3(&Mat3::from_cols(right, up, back));
    shape.blob(
        place(wide * 0.35, tall.min(0.0105) * 0.05, deep + 0.0026),
        Vec3::new(wide * 0.9 + 0.0042, tall.min(0.0105) * 1.08, 0.0064),
        turn,
    );
    forearm(&mut shape, place(wide * 0.4, -tall * 0.35, deep + 0.004), arm, reach);
    shape
}

/// A left hand cupped under a barrel that runs ahead through `middle` and is `radius` thick: the
/// fingers go under it and up the far side, the thumb comes up the near side, and the forearm
/// leaves along `arm`. With `pointing`, the first finger lies along the barrel instead.
fn cradle(middle: Vec3, radius: f32, pointing: bool, arm: Vec3, reach: f32) -> Shape {
    let f = FINGER;
    // Round the barrel, measured anticlockwise from its right side as the player sees it.
    let round = |degrees: f32, out: f32, back: f32| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        middle + Vec3::new(cos, sin, 0.) * (radius + out) + Vec3::Z * back
    };
    let mut shape = Shape::default();
    let reach_round: &[f32] = &[225., 262., 300., 338., 376., 408.];
    let fingers: Vec<(Vec3, f32, f32)> = reach_round
        .iter()
        .map(|degrees| (round(*degrees, f * 0.9, if pointing { 0.001 } else { -0.002 }), f, if pointing { 0.0078 } else { 0.0100 }))
        .collect();
    shape.tube(&fingers, Vec3::Z);
    shape.tube(
        &[
            (round(228., f * 0.85, 0.0075), 0.0031, 0.0031),
            (round(200., f * 0.85, 0.0078), 0.0030, 0.0030),
            (round(172., f * 0.85, 0.0078), 0.0028, 0.0028),
            (round(150., f * 0.8, 0.0074), 0.0025, 0.0025),
        ],
        Vec3::Z,
    );
    if pointing {
        shape.tube(
            &[
                (round(222., f * 0.8, -0.004), 0.0029, 0.0029),
                (round(204., f * 0.85, -0.011), 0.0028, 0.0028),
                (round(192., f * 0.85, -0.018), 0.0027, 0.0027),
                (round(184., f * 0.7, -0.024), 0.0025, 0.0025),
            ],
            Vec3::Y,
        );
    }
    shape.blob(
        round(232., 0.0042, 0.001),
        Vec3::new(0.0088, 0.0058, 0.0110),
        Quat::from_rotation_z((232f32 - 270.).to_radians()),
    );
    forearm(&mut shape, round(235., 0.006, 0.004), arm, reach);
    shape
}

/// A hotdog's forearm: a wrist that leaves the heel of the hand along `arm` and thickens towards
/// an elbow far enough away to stay out of view however hard the weapon kicks.
fn forearm(shape: &mut Shape, wrist: Vec3, arm: Vec3, reach: f32) {
    let arm = arm.normalize();
    // A sausage is never quite straight.
    let sag = Vec3::NEG_Y * 0.007;
    shape.tube(
        &[
            (wrist - arm * 0.003, 0.0060, 0.0060),
            (wrist + arm * 0.012, 0.0064, 0.0064),
            (wrist + arm * 0.05 + sag * 0.25, 0.0082, 0.0082),
            (wrist + arm * reach * 0.6 + sag, 0.0093, 0.0093),
            (wrist + arm * reach + sag * 2.2, 0.0097, 0.0097),
        ],
        Vec3::Y,
    );
}

/// The hands that hold a weapon in first person, in the frame of its view model root: the right
/// hand first, then the left where the weapon is long enough to need one. The numbers are the
/// grips, barrels and buttons of `weapons::model`, in the gun frame (ahead, up, to the right),
/// carried into the view by `weapons::in_view`.
pub fn shapes(weapon: usize) -> Vec<Shape> {
    let view = crate::weapons::in_view(weapon);
    let size = view.scale.x;
    let gun = |ahead: f32, up: f32, side: f32| view.transform_point(Vec3::new(side, up, -ahead));
    // `weapons::Model::grip(front, depth)`: a handle raked back from under the grip point, with
    // the trigger just ahead of it.
    let grip = |front: f32, depth: f32, arm: Vec3| {
        let (top, bottom) = (gun(front - 0.006, -0.003, 0.), gun(front - 0.0105, -depth, 0.));
        fist(
            (top + bottom) * 0.5,
            top - bottom,
            (0.00495 * size, 0.0064 * size, (depth - 0.003) * 0.5 * size),
            Index::Trigger(gun(front + 0.0045, -0.0066, 0.)),
            arm,
            0.16,
        )
    };
    match weapon {
        // Mustard pistol: one hand.
        1 => vec![grip(0.004, 0.024, RIGHT_ARM)],
        // Ketchup sniper: the left hand cups the ribbed shroud behind the bipod.
        2 => vec![grip(0.004, 0.022, RIGHT_ARM), cradle(gun(0.055, 0.001, 0.), 0.0056 * size, false, LEFT_ARM, 0.22)],
        // Hot sauce thrower: its grip is buried in the sauce bottle, so the right hand holds the
        // bottle from underneath with a finger along it, and the left takes the heat shield.
        3 => vec![
            cradle(gun(0.003, -0.0155, 0.) * Vec3::new(-1., 1., 1.), 0.0080 * size, true, RIGHT_ARM * Vec3::new(-1., 1., 1.), 0.16).mirrored(),
            cradle(gun(0.044, -0.0005, 0.), 0.0074 * size, false, LEFT_ARM, 0.22),
        ],
        // Relish bazooka: the left hand is a fist round the foregrip.
        4 => vec![
            grip(0.004, 0.024, RIGHT_ARM),
            fist(
                gun(0.037, -0.0078, 0.) * Vec3::new(-1., 1., 1.),
                Vec3::Y,
                (0.0032 * size, 0.0032 * size, 0.0088 * size),
                Index::Curled,
                LEFT_ARM * Vec3::new(-1., 1., 1.),
                0.22,
            )
            .mirrored(),
        ],
        // Bug spray: the hand is round the can and the first finger is on the button.
        5 => vec![fist(
            gun(0.008, -0.004, 0.),
            Vec3::Y,
            (0.0092 * size, 0.0092 * size, 0.012 * size),
            Index::Button(gun(0.008, 0.0258, 0.)),
            RIGHT_ARM,
            0.16,
        )],
        // Syrup rifle: the right hand on the pistol grip, the left cupping the ribbed handguard
        // halfway along, ahead of the magazine.
        6 => vec![grip(0.004, 0.021, RIGHT_ARM), cradle(gun(0.046, 0.0035, 0.), 0.0065 * size, false, LEFT_ARM, 0.22)],
        // Mayonnaise: one fist round the bottle's body, from underneath, the fingers over its top to squeeze.
        7 => vec![fist(gun(0.001, 0., 0.), Vec3::Z, (0.0100 * size, 0.0100 * size, 0.0115 * size), Index::Curled, RIGHT_ARM, 0.16)],
        _ => vec![],
    }
}

/// The hand meshes, built once: the view models are rebuilt every round.
#[derive(Resource, Default)]
pub struct Hands {
    meshes: [Option<Vec<Handle<Mesh>>>; 8],
}

/// Gives the first-person model of `weapon` its hands, as children of `root`, and marks `root` as
/// a [`ViewModel`]. `arena::build_viewmodels` calls this where it used to hang a plain blob.
pub fn hands(world: &mut World, root: Entity, weapon: usize) {
    let weapon = weapon.min(7);
    world.entity_mut(root).insert(ViewModel(weapon));
    let Some(skin) = world
        .get_resource::<crate::player_model::PlayerModels>()
        .map(|models| models.skin_material.clone())
    else {
        return;
    };
    if world.get_resource::<Hands>().is_none_or(|hands| hands.meshes[weapon].is_none()) {
        let built: Vec<Handle<Mesh>> = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            shapes(weapon).iter().map(|shape| meshes.add(shape.mesh())).collect()
        };
        world.get_resource_or_insert_with(Hands::default).meshes[weapon] = Some(built);
    }
    let meshes = world.resource::<Hands>().meshes[weapon].clone().unwrap_or_default();
    for (i, mesh) in meshes.into_iter().enumerate() {
        let hand = world
            .spawn((
                Name::new(if i == 0 { "First-person right hand" } else { "First-person left hand" }),
                Hand,
                Mesh3d(mesh),
                MeshMaterial3d(skin.clone()),
                Transform::default(),
            ))
            .id();
        world.entity_mut(root).add_child(hand);
    }
}

/// Where this player's hands hold a carried napkin in first person, in the game camera's frame (x to the
/// right, y up, -z ahead): low in the view, a hand either side. The napkin's sheet (`cloth`) hangs from these
/// two points in the world, and `carrying_hands` puts a fist on each.
pub const CARRY_GRIPS: [Vec3; 2] = [Vec3::new(-0.048, -0.058, -0.12), Vec3::new(0.048, -0.058, -0.12)];
/// The two fists that hold a napkin in first person, closed round its gathered edge at CARRY_GRIPS, their
/// forearms running back to the lower corners of the view. They hang from the game's camera under one root,
/// hidden, so that they are drawn on the view model's layer; `arena` shows them while this player carries a
/// napkin and moves them with the sway.
pub fn carrying_hands(world: &mut World, camera: Entity) -> Option<Entity> {
    let skin = world.get_resource::<crate::player_model::PlayerModels>().map(|models| models.skin_material.clone())?;
    let lift = Vec3::Y * 0.006;
    let handle = (0.0048, 0.0048, 0.0085);
    let right = fist(CARRY_GRIPS[1] + lift, Vec3::Y, handle, Index::Curled, RIGHT_ARM, 0.16);
    let left = fist((CARRY_GRIPS[0] + lift) * Vec3::new(-1., 1., 1.), Vec3::Y, handle, Index::Curled, RIGHT_ARM, 0.16).mirrored();
    let meshes: Vec<Handle<Mesh>> = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        [right, left].iter().map(|shape| meshes.add(shape.mesh())).collect()
    };
    let root = world.spawn((Name::new("First-person hands holding a napkin"), Transform::default(), Visibility::Hidden)).id();
    for (i, mesh) in meshes.into_iter().enumerate() {
        let hand = world
            .spawn((Name::new(if i == 0 { "First-person right hand" } else { "First-person left hand" }), Hand, Mesh3d(mesh), MeshMaterial3d(skin.clone()), Transform::default()))
            .id();
        world.entity_mut(root).add_child(hand);
    }
    world.entity_mut(camera).add_child(root);
    Some(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_model::PlayerModels;
    use bevy::pbr::DistanceFog;

    /// A world with the hotdog models in it, as the game has by the time view models are built.
    fn world() -> App {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let models = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, false))
        });
        world.insert_resource(models);
        app
    }
    /// The first-person models as `arena::build_viewmodels` hangs them from the camera: a root per
    /// weapon with the kit's model and the hands, then the stim forearm and a bolt of lightning.
    fn build(world: &mut World) -> (Entity, Vec<Entity>) {
        let camera = world.spawn((HotdogCamera, Transform::default(), Visibility::default())).id();
        let kit = world.resource::<PlayerModels>().weapons.clone();
        let roots: Vec<Entity> = (1..8)
            .map(|weapon| {
                let root = world.spawn((Transform::from_xyz(0.045, -0.047, -0.105), Visibility::Hidden)).id();
                world.entity_mut(camera).add_child(root);
                hands(world, root, weapon);
                kit.attach(world, root, weapon, crate::weapons::in_view(weapon), Visibility::Inherited);
                root
            })
            .collect();
        let arm = world.spawn((Transform::default(), Visibility::Hidden)).id();
        let slap = world.spawn((Mesh3d::default(), Transform::default())).id();
        let bolt = world.spawn((Mesh3d::default(), Transform::default(), Visibility::Hidden)).id();
        world.entity_mut(arm).add_child(slap);
        world.entity_mut(camera).add_children(&[arm, bolt]);
        (camera, roots)
    }
    fn below(world: &World, root: Entity) -> Vec<Entity> {
        let mut all = vec![];
        let mut queue = vec![root];
        while let Some(entity) = queue.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                all.extend(children.iter());
                queue.extend(children.iter());
            }
        }
        all
    }
    fn bounds(shape: &Shape) -> (Vec3, Vec3) {
        shape.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }

    #[test]
    fn everything_under_the_camera_is_drawn_by_the_view_pass_alone() {
        let mut app = world();
        app.add_systems(Update, (layer, light));
        let (camera, _) = build(app.world_mut());
        // A hotdog and a lamp out in the world, and a lamp that has its own idea about layers.
        let world = app.world_mut();
        let other = world.spawn((Mesh3d::default(), Transform::default())).id();
        let lamp = world.spawn((PointLight::default(), Transform::default())).id();
        let spot = world.spawn((SpotLight::default(), Transform::default())).id();
        let private = world.spawn((PointLight::default(), RenderLayers::layer(3))).id();
        app.update();
        let world = app.world();
        let parts = below(world, camera);
        assert!(parts.len() > 30, "the view models are {} entities", parts.len());
        for part in &parts {
            assert_eq!(world.get::<RenderLayers>(*part), Some(&RenderLayers::layer(VIEW_LAYER)), "a part of the view model is on the world's layer");
            assert!(world.get::<NotShadowCaster>(*part).is_some());
        }
        // The camera itself and the world keep the default layer.
        assert!(world.get::<RenderLayers>(camera).is_none() && world.get::<RenderLayers>(other).is_none());
        for light in [lamp, spot] {
            assert_eq!(world.get::<RenderLayers>(light), Some(&RenderLayers::from_layers(&[0, VIEW_LAYER])));
        }
        assert_eq!(world.get::<RenderLayers>(private), Some(&RenderLayers::layer(3)));
        // Something hung from a view model later (a muzzle flash, a new hand) follows on the next frame.
        let world = app.world_mut();
        let late = world.spawn((Mesh3d::default(), Transform::default())).id();
        let root = below(world, camera)[0];
        world.entity_mut(root).add_child(late);
        app.update();
        assert_eq!(app.world().get::<RenderLayers>(late), Some(&RenderLayers::layer(VIEW_LAYER)));
    }

    #[test]
    fn every_gun_has_its_hands_under_its_own_root() {
        let mut app = world();
        let (_, roots) = build(app.world_mut());
        let world = app.world_mut();
        for (root, weapon) in roots.iter().zip(1..8) {
            assert_eq!(world.get::<ViewModel>(*root).map(|model| model.0), Some(weapon));
            let children: Vec<Entity> = world.get::<Children>(*root).unwrap().iter().collect();
            let held: Vec<Entity> = children.iter().copied().filter(|child| world.get::<Hand>(*child).is_some()).collect();
            let long = matches!(weapon, 2..=4 | 6);
            assert_eq!(held.len(), if long { 2 } else { 1 }, "weapon {weapon}");
            for hand in &held {
                // A hand sits in the root's own frame, so it goes wherever the root is moved or kicked.
                assert_eq!(*world.get::<Transform>(*hand).unwrap(), Transform::IDENTITY);
                assert!(world.get::<Mesh3d>(*hand).is_some() && world.get::<MeshMaterial3d<StandardMaterial>>(*hand).is_some());
            }
            // The weapon itself is still there beside them.
            assert_eq!(children.len(), held.len() + 1);
        }
        // A second round reuses the meshes.
        let before = world.resource::<Assets<Mesh>>().len();
        build(world);
        assert_eq!(world.resource::<Assets<Mesh>>().len(), before);
    }

    #[test]
    fn hands_are_closed_solid_shapes_that_face_outwards() {
        for weapon in 1..8 {
            for (i, shape) in shapes(weapon).iter().enumerate() {
                assert!(shape.positions.len() == shape.normals.len() && shape.positions.len() == shape.uvs.len());
                assert!(shape.indices.len() / 3 > 1500 && shape.indices.len() / 3 < 6000, "weapon {weapon} hand {i}: {} triangles", shape.indices.len() / 3);
                assert!(shape.positions.iter().all(|p| p.is_finite()));
                assert!(shape.normals.iter().all(|n| (n.length() - 1.).abs() < 1e-3));
                for t in shape.indices.chunks(3) {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| shape.positions[i as usize]);
                    let out: Vec3 = t.iter().map(|i| shape.normals[*i as usize]).sum();
                    assert!((b - a).cross(c - a).dot(out) > 0., "weapon {weapon} hand {i}: a triangle faces inwards");
                }
                assert!(shape.mesh().attribute(Mesh::ATTRIBUTE_TANGENT).is_some(), "the skin's normal map needs tangents");
            }
        }
    }

    #[test]
    fn the_right_hand_is_round_the_grip_and_its_arm_comes_from_the_lower_right() {
        for weapon in 1..8 {
            let hand = &shapes(weapon)[0];
            let (lo, hi) = bounds(hand);
            let grip = crate::weapons::in_view(weapon).translation;
            // The grip point is inside the hand's reach, and the hand is no bigger than a hand.
            let near: Vec<Vec3> = hand.positions.iter().copied().filter(|p| p.distance(grip) < 0.045).collect();
            let (close, far) = near.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            assert!(close.x < grip.x && far.x > grip.x && close.z < grip.z + 0.004 && far.z > grip.z, "weapon {weapon}: the hand is beside the grip, not round it");
            assert!(close.y < grip.y - 0.01, "weapon {weapon}: the hand is below the grip point, on the handle");
            // The forearm runs back, down and out to the right, well past the edge of the view.
            let elbow = hand.positions.iter().copied().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
            assert!(elbow.x > grip.x + 0.11 && elbow.y < grip.y - 0.05 && elbow.z > grip.z + 0.05, "weapon {weapon}: elbow at {elbow}");
            assert!(hi.x - lo.x < 0.19 && lo.x > grip.x - 0.03, "weapon {weapon}: nothing of the right hand strays left");
        }
    }

    #[test]
    fn the_trigger_finger_reaches_the_trigger_and_the_spray_finger_the_button() {
        for (weapon, front) in [(1, 0.004), (2, 0.004), (4, 0.004), (6, 0.004)] {
            let view = crate::weapons::in_view(weapon);
            let face = view.transform_point(Vec3::new(0., -0.0066, -(front + 0.0045)));
            let nearest = shapes(weapon)[0].positions.iter().map(|p| p.distance(face)).fold(f32::MAX, f32::min);
            assert!(nearest < 0.002, "weapon {weapon}: the fingertip is {nearest} from the trigger");
            // Nothing of the hand is in front of the trigger guard.
            let guard = view.transform_point(Vec3::new(0., 0., -(front + 0.014))).z;
            let fingers = shapes(weapon)[0].positions.iter().filter(|p| p.x < view.translation.x + 0.012).map(|p| p.z).fold(f32::MAX, f32::min);
            assert!(fingers > guard, "weapon {weapon}: a finger pokes out past the trigger guard");
        }
        let view = crate::weapons::in_view(5);
        let button = view.transform_point(Vec3::new(0., 0.0258, -0.008));
        let hand = &shapes(5)[0];
        let nearest = hand.positions.iter().map(|p| p.distance(button)).fold(f32::MAX, f32::min);
        assert!(nearest < 0.002, "the finger is {nearest} from the spray button");
        assert!(hand.positions.iter().any(|p| p.y > button.y), "the finger is on top of the button");
    }

    #[test]
    fn long_guns_get_a_left_hand_under_the_barrel_and_short_ones_do_not() {
        assert!(shapes(1).len() == 1 && shapes(5).len() == 1 && shapes(0).is_empty());
        for (weapon, ahead, up) in [(2, 0.055, 0.001), (3, 0.044, -0.0005), (4, 0.037, -0.0078), (6, 0.046, 0.0035)] {
            let view = crate::weapons::in_view(weapon);
            let held = view.transform_point(Vec3::new(0., up, -ahead));
            let hand = &shapes(weapon)[1];
            let fingers: Vec<Vec3> = hand.positions.iter().copied().filter(|p| p.distance(held) < 0.03).collect();
            let (lo, hi) = fingers.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            assert!(lo.x < held.x && hi.x > held.x && lo.y < held.y - 0.004, "weapon {weapon}: the left hand is not under the barrel");
            // Well ahead of the right hand, and its forearm comes up from below on the left.
            assert!(held.z < view.translation.z - 0.02);
            let elbow = hand.positions.iter().copied().min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
            assert!(elbow.x < held.x - 0.08 && elbow.y < held.y - 0.1 && elbow.z > held.z + 0.1, "weapon {weapon}: elbow at {elbow}");
        }
    }

    #[test]
    fn the_weapon_is_shaded_when_a_block_hides_the_sun() {
        use crate::ragdoll::{Block, Scene};
        let scene = Scene { blocks: vec![Block { min: Vec3::new(-0.1, 0.9, 2.9), max: Vec3::new(0.1, 1.2, 3.1) }], wedges: vec![] };
        let to_sun = Vec3::new(-0.7, 0.6, -0.3).normalize();
        // Behind the block as the sun sees it, beside it, above it, and on its sunny side.
        let behind = Vec3::new(0., 1.0, 3.0) - to_sun * 0.25;
        assert!(shaded(&scene, behind, to_sun));
        assert!(!shaded(&scene, behind + Vec3::Z * 0.4, to_sun));
        assert!(!shaded(&scene, behind + Vec3::Y * 0.4, to_sun));
        assert!(!shaded(&scene, Vec3::new(0., 1.0, 3.0) + to_sun * 0.3, to_sun));
        // The bundled map, as `death` reads it: out in the open the sun reaches the counter.
        let map = kitchen_sim::map::Map::bundled();
        let scene = crate::death::scenery(&map);
        let bounds = crate::visuals::ArenaBounds::default();
        let open = (0..40)
            .filter(|i| {
                let at = Vec3::new(bounds.min.x + 0.09 * *i as f32, bounds.floor + 0.11, bounds.min.y + 0.1);
                !shaded(&scene, at, -bounds.sunlight())
            })
            .count();
        assert!(open > 20, "the sun reaches {open} of 40 places along the front of the counter");
    }

    #[test]
    fn the_view_camera_takes_over_the_finish_of_the_picture() {
        let mut app = world();
        app.add_systems(Update, (stack, follow).chain());
        let world = app.world_mut();
        let player = world.spawn(Transform::from_xyz(1., 2., 3.).with_rotation(Quat::from_rotation_y(0.5))).id();
        let game = world
            .spawn((
                HotdogCamera,
                Camera3d::default(),
                Msaa::default(),
                Tonemapping::TonyMcMapface,
                Exposure { ev100: 5.5 },
                Transform::from_xyz(0., 0.03, 0.),
            ))
            .id();
        world.entity_mut(player).add_child(game);
        app.update();
        // The game's camera was graded before the view camera existed.
        let graded = ColorGrading::with_identical_sections(
            bevy::render::view::ColorGradingGlobal { post_saturation: 1.08, ..default() },
            bevy::render::view::ColorGradingSection { contrast: 1.06, ..default() },
        );
        app.world_mut().entity_mut(game).insert(graded);
        // First frame: the view camera exists but is idle, and the game's camera is untouched.
        let world = app.world_mut();
        let view = world.query_filtered::<Entity, With<ViewCamera>>().single(world).unwrap();
        assert!(!world.get::<Camera>(view).unwrap().is_active);
        assert!(*world.get::<Tonemapping>(game).unwrap() == Tonemapping::TonyMcMapface);
        // `visuals::dress_cameras` gives the game's camera the look.
        world.entity_mut(view).insert(ColorGrading::default());
        world.entity_mut(game).insert((
            Hdr,
            Msaa::Off,
            Bloom::NATURAL,
            Vignette::default(),
            AmbientLight { brightness: 2., ..default() },
        ));
        app.update();
        app.update();
        let world = app.world();
        let (lens, camera) = (world.get::<Camera>(view).unwrap(), world.get::<Camera>(game).unwrap());
        assert!(lens.is_active && lens.order > camera.order);
        assert!(matches!(lens.clear_color, ClearColorConfig::None), "the view camera must not clear the world away");
        assert!(matches!(camera.output_mode, CameraOutputMode::Skip) && matches!(lens.output_mode, CameraOutputMode::Write { .. }));
        // Tone mapping, bloom and the vignette happen once, at the end.
        assert!(*world.get::<Tonemapping>(game).unwrap() == Tonemapping::None);
        assert!(*world.get::<Tonemapping>(view).unwrap() == Tonemapping::TonyMcMapface);
        assert!(world.get::<Bloom>(game).is_none() && world.get::<Vignette>(game).is_none());
        assert!(world.get::<Bloom>(view).is_some() && world.get::<Vignette>(view).is_some());
        // Same picture, same light, no fog of its own.
        assert!(world.get::<Hdr>(view).is_some() && *world.get::<Msaa>(view).unwrap() == Msaa::Off);
        assert_eq!(world.get::<Exposure>(view).unwrap().ev100, 5.5);
        assert_eq!(world.get::<AmbientLight>(view).unwrap().brightness, 2.);
        let grade = world.get::<ColorGrading>(view).unwrap();
        assert!(grade.global.post_saturation == 1.08 && grade.shadows.contrast == 1.06, "the picture is graded as the game's camera was");
        assert!(world.get::<DistanceFog>(view).is_none());
        assert_eq!(world.get::<RenderLayers>(view), Some(&RenderLayers::layer(VIEW_LAYER)));
        // It stands exactly where the game's camera does, parent and all.
        let place = *world.get::<Transform>(view).unwrap();
        let expected = world.get::<Transform>(player).unwrap().mul_transform(*world.get::<Transform>(game).unwrap());
        assert!(place.translation.distance(expected.translation) < 1e-6 && place.rotation.angle_between(expected.rotation) < 1e-5);
        // A new theme dresses the game's camera again; the finish moves on again.
        let world = app.world_mut();
        world.entity_mut(game).insert(Bloom { intensity: 0.27, ..Bloom::NATURAL });
        world.get_mut::<AmbientLight>(game).unwrap().brightness = 4.;
        app.update();
        let world = app.world();
        assert!(world.get::<Bloom>(game).is_none());
        assert_eq!(world.get::<Bloom>(view).unwrap().intensity, 0.27);
        assert_eq!(world.get::<AmbientLight>(view).unwrap().brightness, 4.);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! How the kitchen looks: the sun on the ceiling and the shadows it casts, haze, light shafts, bloom,
//! colour grading, generated surface detail, and three themes that all of it follows.
//!
//! * `kitchen`: daytime. A large warm sun on the ceiling lights the counter.
//! * `night`: the same room with the disc as a cool moon, deep blue air and strings of small warm bulbs.
//! * `jungle`: a canopy under the ceiling breaks green-gold light into dapples; plants crowd the counter.
//!
//! The theme is the [`Theme`] resource. It starts from `--theme night|jungle` (natively) or
//! `?theme=night|jungle` (in the browser), and changing the resource at any time re-dresses the scene;
//! F6 steps through the three. Everything is placed from [`ArenaBounds`], never from where the game
//! happens to put its blocks.
//!
//! Light is drawn as many fine parts rather than a few large ones: the sun's beam is thousands of
//! hair-thin additive rays gathered into sheaves, its corona is fine radial spikes, the counter
//! twinkles with glints, the air carries dust motes, and fill light comes from rings and strings of
//! small lamps. All of those are meshes of ribbons (see `filaments`), so they cost draw calls, not lights.
//!
//! Nothing here spawns or edits the game's own scene. The systems below find the camera, the lights
//! and the meshes after the game has spawned them and adjust them in place.
//!
//! Options, as `--name value` natively and `?name=value` in the browser: `theme`, `quality` (see
//! `quality`), `photo` (`x,y,z,tx,ty,tz[,fov]`, a fixed camera for screenshots), and `sharpen` (0 to 1,
//! the sharpening of a picture drawn under the screen's resolution, instead of what its scale calls for).
//!
//! How much of all this is drawn is the [`Detail`] that `quality` settles on for the device, and
//! everything here follows it when it changes, without a reload. Its feature tier is one of three
//! [`Quality`] sets: native and WebGPU builds get everything; the WebGL 2 set drops the effects WebGL 2
//! cannot run (volumetric fog, ambient occlusion, temporal anti-aliasing, contact shadows); the minimal
//! set keeps only the lighting and a thinned-out set of rays and plants. Within a set the detail
//! decides the shadow map, multisampling (or FXAA, `viewmodel`), HDR and bloom, fog, reflections, the film grain,
//! the kitchen lamps' shadows and how many rays, plants and lamps there are.
use std::f32::consts::TAU;

use bevy::{
    anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, taa::TemporalAntiAliasing},
    camera::{visibility::NoFrustumCulling, Hdr},
    core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, NormalPrepass},
    light::{
        cluster::ClusterConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap, FogVolume, NotShadowCaster,
        ShadowFilteringMethod, VolumetricFog, VolumetricLight,
    },
    pbr::{ContactShadows, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion},
    post_process::{
        bloom::Bloom,
        effect_stack::{ChromaticAberration, Vignette},
    },
    prelude::*,
    render::{
        camera::{MipBias, TemporalJitter},
        renderer::{RenderAdapterInfo, RenderDevice},
        view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    },
    ui::widget::NodeImageMode,
};

mod filaments;
mod foliage;
mod sharpen;
mod surfaces;

use crate::quality::Detail;
use filaments::{Dice, FilamentMaterial, Filaments};
pub(crate) use surfaces::Surveyed;

/// The look of the whole scene. Change it and everything follows on the next frame.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Kitchen,
    Night,
    Jungle,
}

impl Theme {
    /// Reads a theme's name as a map file or the command line spells it.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "kitchen" | "day" => Some(Self::Kitchen),
            "night" => Some(Self::Night),
            "jungle" => Some(Self::Jungle),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Kitchen => "kitchen",
            Self::Night => "night",
            Self::Jungle => "jungle",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Kitchen => Self::Night,
            Self::Night => Self::Jungle,
            Self::Jungle => Self::Kitchen,
        }
    }
}

/// Switches the look at runtime. Equivalent to writing the [`Theme`] resource.
#[allow(dead_code)]
pub fn set_theme(world: &mut World, theme: Theme) {
    if *world.resource::<Theme>() != theme {
        *world.resource_mut::<Theme>() = theme;
    }
}

/// The playable rectangle of counter, in world metres: the one place the sun, the lights, the fog and
/// the foliage take their positions from. `min` and `max` are (x, z).
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct ArenaBounds {
    pub min: Vec2,
    pub max: Vec2,
    /// Height of the counter top.
    pub floor: f32,
    /// Height of the underside of the ceiling.
    pub ceiling: f32,
    /// The map stands in a room of its own, its ground the kitchen floor (`Map::floor`, floor.rs), which lights itself
    /// (the moon through its window, the fridge): the theme leaves out its disc on the ceiling, its rays and its lamps.
    pub room: bool,
}

impl Default for ArenaBounds {
    fn default() -> Self {
        Self { min: Vec2::new(-1.8, 2.27), max: Vec2::new(1.8, 3.63), floor: 0.9025, ceiling: 2.55, room: false }
    }
}

impl ArenaBounds {
    /// The middle of the counter top.
    pub fn centre(&self) -> Vec3 {
        Vec3::new((self.min.x + self.max.x) / 2., self.floor, (self.min.y + self.max.y) / 2.)
    }

    /// Where the sun sits: on the ceiling, off to the left of the counter and a little in front of it,
    /// so its light rakes across the counter from the side and every shadow falls where the cameras,
    /// which look at the counter from the front, can see it.
    pub fn sun(&self) -> Vec3 {
        let centre = self.centre();
        Vec3::new(centre.x - 2.0, self.ceiling - 0.012, centre.z - 0.85)
    }

    /// The direction the sun's light travels.
    pub fn sunlight(&self) -> Vec3 {
        (self.centre() + Vec3::Z * 0.1 - self.sun()).normalize()
    }
}

/// The feature set the scene is dressed with; `quality` picks it with the rest of the [`Detail`].
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Quality {
    /// Everything: native builds and the WebGPU browser build, at High.
    Full,
    /// What WebGL 2 can run: the WebGL 2 build at High, and every build at Medium and Low.
    Lite,
    /// Lighting and little else: a CPU renderer, and every build at Lowest.
    Minimal,
}

/// Everything the current theme spawned. Despawned and rebuilt when the theme changes.
#[derive(Component)]
pub struct ThemeOwned;

const SUN_RADIUS: f32 = 0.33;

/// Whether Bevy was built with `pbr_anisotropy_texture`: natively and with the `webgpu` feature.
const ANISOTROPY: bool = cfg!(any(not(target_arch = "wasm32"), feature = "webgpu"));

#[derive(Resource, Clone, Debug, Default)]
struct VisualOptions {
    theme: Theme,
    /// Whether a theme was asked for on the command line or in the page address; it then wins over the map's.
    chosen: bool,
    /// Experimental opt-in; see `report_raytracing`.
    raytraced: bool,
    /// A fixed camera for looking at the scene: eye, target, vertical field of view in degrees.
    photo: Option<[f32; 7]>,
    /// `--sharpen` / `?sharpen=`: the sharpening of a stretched picture at this strength (0 to 1, 0 for none)
    /// instead of the one its scale calls for ([`sharpening`]).
    sharpness: Option<f32>,
}

impl VisualOptions {
    fn read() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (theme, photo, raytraced, sharpness) = {
            let args: Vec<String> = std::env::args().collect();
            let value = |key: &str| {
                args.iter()
                    .position(|s| s == key)
                    .and_then(|i| args.get(i + 1))
                    .cloned()
                    .unwrap_or_default()
            };
            (value("--theme"), value("--photo"), args.iter().any(|s| s == "--raytraced"), value("--sharpen"))
        };
        #[cfg(target_arch = "wasm32")]
        let (theme, photo, raytraced, sharpness) = {
            let v: serde_json::Value = serde_json::from_str(&visual_options()).unwrap_or_default();
            let text = |key: &str| v[key].as_str().unwrap_or("").to_string();
            (text("theme"), text("photo"), v["raytraced"].as_bool().unwrap_or(false), text("sharpen"))
        };

        let numbers: Vec<f32> = photo.split(',').filter_map(|n| n.trim().parse().ok()).collect();
        Self {
            chosen: Theme::parse(&theme).is_some(),
            theme: Theme::parse(&theme).unwrap_or_default(),
            raytraced,
            sharpness: sharpness.trim().parse::<f32>().ok().filter(|s| s.is_finite()).map(|s| s.clamp(0., 1.)),
            photo: (numbers.len() >= 6).then(|| {
                let mut shot = [0., 0., 0., 0., 0., 0., 60.];
                shot[..numbers.len().min(7)].copy_from_slice(&numbers[..numbers.len().min(7)]);
                shot
            }),
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function visual_options(){const q=new URLSearchParams(location.search);return JSON.stringify({theme:q.get('theme')||'',photo:q.get('photo')||'',raytraced:q.has('raytraced'),sharpen:q.get('sharpen')||''});}
")]
extern "C" {
    fn visual_options() -> String;
}

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        let options = VisualOptions::read();
        app.add_plugins((MaterialPlugin::<FilamentMaterial>::default(), crate::quality::Plugin, sharpen::SharpenPlugin))
            .insert_resource(options.theme)
            .insert_resource(options)
            .init_resource::<ArenaBounds>()
            .insert_resource(crate::quality::best())
            .init_resource::<Stage>()
            .add_systems(
                Update,
                (
                    prepare,
                    follow_detail,
                    follow_map,
                    cycle_theme,
                    surfaces::upgrade,
                    apply_theme,
                    dress_cameras,
                    sharpen,
                    crate::player_model::follow_detail,
                    tune_room_lights,
                    lamp_shadows,
                    follow_intro,
                    pace_volumetrics,
                    foliage::sway,
                    drift_grain,
                )
                    .chain()
                    .after(crate::quality::Decide),
            )
            .add_systems(
                PostUpdate,
                photo_camera.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// The map being played names its look. A theme asked for with `--theme` or `?theme=` wins over it.
fn follow_map(map: Option<Res<crate::level::MapTheme>>, options: Res<VisualOptions>, mut theme: ResMut<Theme>) {
    let Some(map) = map else { return };
    if !map.is_changed() || options.chosen {
        return;
    }
    if let Some(wanted) = Theme::parse(&map.0) {
        if *theme != wanted {
            *theme = wanted;
        }
    }
}

/// What the theme's own entities are built for: the tier and the parts of the detail they depend on
/// (shares of rays and plants in hundredths, whether the sun casts shadows, the lamps).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Shape {
    tier: Quality,
    rays: u32,
    plants: u32,
    shadows: bool,
    lamps: usize,
}

impl Shape {
    fn of(detail: &Detail) -> Self {
        Shape {
            tier: detail.tier,
            rays: (detail.rays * 100.).round() as u32,
            plants: (detail.plants * 100.).round() as u32,
            shadows: detail.shadow_map > 0 && detail.tier != Quality::Minimal,
            lamps: detail.lamps,
        }
    }
}

/// What has been set up so far.
#[derive(Resource, Default)]
struct Stage {
    ready: bool,
    /// Ambient occlusion needs more storage textures in a shader than every GPU offers.
    occlusion: bool,
    applied: Option<(Theme, ArenaBounds, Shape)>,
    /// The tier the generated maps were made for.
    library: Option<Quality>,
    environment: Option<surfaces::Environment>,
    /// Cameras take the new theme's settings on the next pass.
    restyle: bool,
}

/// Every number that differs between the three themes.
struct Look {
    /// The sun or moon: light colour, illuminance in lux, and the colour of its disc on screen.
    sun: Color,
    lux: f32,
    disc: Vec3,
    /// Fill light: a flat ambient term and the strength of the reflected room.
    ambient: Color,
    nits: f32,
    environment: f32,
    /// Haze: its colour (also what lies beyond the room), density per metre, and its glow toward the sun.
    haze: Color,
    density: f32,
    glow: Color,
    /// The lit volume of air, where the renderer can march through it.
    air: f32,
    air_ambient: f32,
    bloom: f32,
    /// Grading: exposure in stops, colour temperature, saturation, contrast.
    grade: [f32; 4],
    vignette: f32,
    /// What is left of the kitchen's own ceiling spot once the round starts, and of its other lamps.
    room: f32,
    fill: f32,
    /// Filament colours with their intensity in the fourth number: rays, corona, glints, motes.
    rays: Vec4,
    corona: Vec4,
    glints: Vec4,
    motes: Vec4,
}

impl Theme {
    fn look(self) -> Look {
        match self {
            Theme::Kitchen => Look {
                sun: Color::srgb(1.0, 0.93, 0.80),
                lux: 1300.,
                disc: Vec3::new(60., 43., 17.),
                ambient: Color::srgb(0.85, 0.90, 1.0),
                nits: 2.,
                environment: 17.,
                haze: Color::srgb(0.72, 0.71, 0.68),
                density: 0.035,
                glow: Color::srgb(1.0, 0.85, 0.55),
                air: 0.007,
                air_ambient: 0.004,
                bloom: 0.17,
                grade: [0., 0.02, 1.08, 1.06],
                vignette: 0.32,
                room: 0.22,
                fill: 0.5,
                rays: Vec4::new(1.0, 0.82, 0.50, 0.36),
                corona: Vec4::new(1.0, 0.72, 0.30, 6.),
                glints: Vec4::new(1.0, 0.90, 0.65, 0.40),
                motes: Vec4::new(1.0, 0.90, 0.70, 0.55),
            },
            Theme::Night => Look {
                sun: Color::srgb(0.60, 0.72, 1.0),
                lux: 260.,
                disc: Vec3::new(5.5, 7., 10.),
                ambient: Color::srgb(0.30, 0.42, 0.90),
                nits: 4.,
                environment: 38.,
                haze: Color::srgb(0.014, 0.024, 0.060),
                density: 0.20,
                glow: Color::srgb(0.25, 0.35, 0.70),
                air: 0.03,
                air_ambient: 0.004,
                bloom: 0.27,
                grade: [0.1, -0.04, 1.12, 1.10],
                vignette: 0.52,
                room: 0.025,
                fill: 0.12,
                rays: Vec4::new(0.55, 0.70, 1.0, 0.20),
                corona: Vec4::new(0.55, 0.70, 1.0, 1.2),
                glints: Vec4::new(0.60, 0.75, 1.0, 0.16),
                motes: Vec4::new(0.60, 0.75, 1.0, 0.22),
            },
            Theme::Jungle => Look {
                sun: Color::srgb(1.0, 0.94, 0.72),
                lux: 1700.,
                disc: Vec3::new(60., 47., 15.),
                ambient: Color::srgb(0.55, 0.85, 0.45),
                nits: 3.,
                environment: 26.,
                haze: Color::srgb(0.36, 0.46, 0.30),
                density: 0.08,
                glow: Color::srgb(1.0, 0.92, 0.50),
                air: 0.014,
                air_ambient: 0.004,
                bloom: 0.20,
                grade: [0., 0.03, 1.15, 1.05],
                vignette: 0.42,
                room: 0.10,
                fill: 0.35,
                rays: Vec4::new(1.0, 0.94, 0.55, 0.26),
                corona: Vec4::new(1.0, 0.85, 0.35, 2.8),
                glints: Vec4::new(0.95, 1.0, 0.55, 0.42),
                motes: Vec4::new(0.90, 1.0, 0.55, 0.55),
            },
        }
    }
}

/// Runs once the renderer is known and `quality` has settled the detail: generates the textures.
fn prepare(
    mut commands: Commands,
    adapter: Option<Res<RenderAdapterInfo>>,
    device: Option<Res<RenderDevice>>,
    options: Res<VisualOptions>,
    detail: Option<Res<Detail>>,
    mut stage: ResMut<Stage>,
    quality: Res<Quality>,
    mut images: ResMut<Assets<Image>>,
) {
    if stage.ready {
        return;
    }
    let (Some(adapter), Some(detail)) = (adapter, detail) else {
        return;
    };
    stage.ready = true;
    stage.occlusion = device.is_some_and(|d| d.limits().max_storage_textures_per_shader_stage >= 5);
    info!(
        "Visuals: {} theme, {:?} tier on {} ({:?}, {:?}){}",
        options.theme.name(),
        *quality,
        adapter.name,
        adapter.device_type,
        adapter.backend,
        if stage.occlusion { "" } else { ", no ambient occlusion" },
    );
    if options.raytraced && !crate::ultra::has_raytracing() {
        report_raytracing();
    }
    commands.insert_resource(DirectionalLightShadowMap { size: detail.shadow_map.max(512) as usize });
    commands.insert_resource(surfaces::Library::new(&mut images, *quality));
    stage.library = Some(*quality);
    // Film grain over everything, behind the HUD; and a vignette where post-processing has none. The
    // detail shows them or not (`follow_detail`).
    commands.spawn((
        Grain,
        Overlay,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.),
            top: Val::Px(0.),
            width: Val::Px(4352.),
            height: Val::Px(2560.),
            ..default()
        },
        ImageNode {
            image: images.add(surfaces::grain_image(0.024)),
            image_mode: NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1. },
            ..default()
        },
        GlobalZIndex(-100),
    ));
    commands.spawn((
        Overlay,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.),
            top: Val::Px(0.),
            width: Val::Percent(100.),
            height: Val::Percent(100.),
            ..default()
        },
        ImageNode {
            image: images.add(surfaces::vignette_image(0.5)),
            image_mode: NodeImageMode::Stretch,
            ..default()
        },
        GlobalZIndex(-101),
    ));
}

/// Film grain and the vignette drawn over the picture (the vignette only where post-processing has none).
#[derive(Component)]
struct Overlay;

/// Follows a change of the detail in what is not rebuilt with the theme: the shadow map, the overlay,
/// the generated maps (made again, finer, when the tier rises from the minimal set, which had coarse
/// ones and dressed nothing) and the cameras.
fn follow_detail(
    mut commands: Commands,
    detail: Option<Res<Detail>>,
    quality: Res<Quality>,
    mut stage: ResMut<Stage>,
    mut images: ResMut<Assets<Image>>,
    shadow_map: Option<ResMut<DirectionalLightShadowMap>>,
    mut overlays: Query<(&mut Visibility, Has<Grain>), With<Overlay>>,
    surveyed: Query<Entity, (With<Surveyed>, Without<ThemeOwned>)>,
    mut seen: Local<Option<Detail>>,
) {
    let Some(detail) = detail else { return };
    if !stage.ready || seen.as_ref() == Some(&*detail) {
        return;
    }
    let before = seen.replace(*detail);
    if let Some(mut map) = shadow_map {
        let size = detail.shadow_map.max(512) as usize;
        if map.size != size {
            map.size = size;
        }
    }
    for (mut visibility, grain) in &mut overlays {
        let shown = detail.overlay && *quality != Quality::Minimal && (grain || *quality == Quality::Lite);
        let wanted = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    if stage.library == Some(Quality::Minimal) && *quality != Quality::Minimal {
        commands.insert_resource(surfaces::Library::new(&mut images, *quality));
        stage.library = Some(*quality);
        for entity in &surveyed {
            commands.entity(entity).try_remove::<Surveyed>();
        }
    }
    // The cameras are dressed again only for what they carry: not for a new render scale.
    let lens = |d: &Detail| (d.tier, d.msaa, d.hdr, d.fog, d.reflections, d.soft_shadows);
    if before.is_some_and(|b| lens(&b) != lens(&detail)) {
        stage.restyle = true;
    }
}

/// `--raytraced` / `?raytraced` in a build without the Ultra tier: Bevy 0.19's raytraced lighting (`bevy_solari`) is
/// compiled only into native builds with the `ultra` feature (ultra.rs), which report on it themselves.
fn report_raytracing() {
    warn!(
        "Visuals: --raytraced was asked for, but this build has no raytraced lighting. It is the native Ultra tier's: \
         build with `cargo run --release --features ultra -- --raytraced` (README, \"Ultra tier (native)\"); it needs \
         a Vulkan or DX12 GPU with ray queries. The rasterised lighting is used instead."
    );
}

#[derive(Component)]
struct Grain;

fn drift_grain(theme: Res<Theme>, mut grain: Query<(&mut Node, &mut ImageNode), With<Grain>>, mut dice: Local<u32>) {
    // Grain shows most against black, so the night gets less of it.
    let strength = if *theme == Theme::Night { 0.45 } else { 1. };
    for (mut node, mut image) in &mut grain {
        if image.color.alpha() != strength {
            image.color = Color::WHITE.with_alpha(strength);
        }
        *dice = dice.wrapping_mul(1664525).wrapping_add(1013904223);
        node.left = Val::Px(-(((*dice >> 8) % 256) as f32));
        node.top = Val::Px(-(((*dice >> 20) % 256) as f32));
    }
}

fn cycle_theme(keys: Res<ButtonInput<KeyCode>>, mut theme: ResMut<Theme>) {
    if keys.just_pressed(KeyCode::F6) {
        *theme = theme.next();
        info!("Visuals: {} theme", theme.name());
    }
}

/// Scales a light with the opening of the refrigerator door in the intro, so none of this lights the
/// inside of a closed fridge. Holds the light's full illuminance.
#[derive(Component)]
struct IntroScaled(f32);

fn filament_material(color: Vec4, motion: [f32; 4], fade: [f32; 4], widest: f32) -> FilamentMaterial {
    FilamentMaterial {
        color,
        motion: Vec4::from_array(motion),
        fade: Vec4::from_array(fade),
        limits: Vec4::new(widest, 0., 0., 0.),
    }
}

/// Rebuilds everything the theme owns when the theme, the arena, the tier or the shares of rays and
/// plants change.
fn apply_theme(
    mut commands: Commands,
    theme: Res<Theme>,
    bounds: Res<ArenaBounds>,
    detail: Option<Res<Detail>>,
    mut stage: ResMut<Stage>,
    library: Option<Res<surfaces::Library>>,
    owned: Query<Entity, With<ThemeOwned>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut filaments: ResMut<Assets<FilamentMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(detail) = detail else { return };
    let key = (*theme, *bounds, Shape::of(&detail));
    if !stage.ready || stage.applied == Some(key) {
        return;
    }
    stage.applied = Some(key);
    stage.restyle = true;
    for entity in &owned {
        commands.entity(entity).despawn();
    }
    let (theme, bounds, shape) = key;
    let quality = shape.tier;
    let look = theme.look();
    let (sun, sunlight, centre) = (bounds.sun(), bounds.sunlight(), bounds.centre());
    let size = bounds.max - bounds.min;
    // Counts for the three tiers, and for rays, glints and motes the detail's share of them.
    let tier = |full: usize, lite: usize, minimal: usize| match quality {
        Quality::Full => full,
        Quality::Lite => lite,
        Quality::Minimal => minimal,
    };
    let rays = |full: usize, lite: usize, minimal: usize| (tier(full, lite, minimal) as f32 * detail.rays).round() as usize;

    if quality != Quality::Minimal {
        stage.environment = Some(surfaces::environment(&mut images, theme, -sunlight, 64));
    }

    // The sun: one directional light, the only light in the theme that casts shadows.
    let mut light = commands.spawn((
        DirectionalLight {
            color: look.sun,
            illuminance: 0.,
            shadow_maps_enabled: shape.shadows,
            contact_shadows_enabled: quality == Quality::Full,
            // Metres, on characters 15 cm tall: the default of 2 cm would lift every shadow off its feet.
            shadow_depth_bias: 0.004,
            shadow_normal_bias: 1.0,
            ..default()
        },
        // The whole scene is a few metres across: spend the cascades on the first metre and a half
        // in front of the camera, where a first-person hotdog looks at shadows from centimetres away.
        // The distances suit the bundled counter (its diagonal is just under 3.85 m); a bigger one
        // stretches them in proportion, so that shadows still reach across all of it.
        if quality == Quality::Full {
            CascadeShadowConfigBuilder {
                num_cascades: 4,
                minimum_distance: 0.01,
                first_cascade_far_bound: 0.55,
                maximum_distance: 7. * (size.length() / 3.85).max(1.),
                overlap_proportion: 0.15,
            }
        } else {
            CascadeShadowConfigBuilder {
                num_cascades: 1,
                minimum_distance: 0.01,
                first_cascade_far_bound: 3.4 * (size.length() / 3.85).max(1.),
                maximum_distance: 3.4 * (size.length() / 3.85).max(1.),
                overlap_proportion: 0.1,
            }
        }
        .build(),
        Transform::from_translation(sun).looking_to(sunlight, Vec3::Y),
        // Without shadows or tone mapping headroom the full sun would bleach a software-rendered scene.
        // (A room of its own is lit by its own moon through its window: the theme's sun is out there.)
        IntroScaled(look.lux * if quality == Quality::Minimal { 0.5 } else { 1. } * if bounds.room { 0. } else { 1. }),
        ThemeOwned,
    ));
    if quality == Quality::Full {
        light.insert(VolumetricLight);
    }

    // The disc itself: a shallow glowing dome on the ceiling, far brighter than anything else in view. (Not in a room of
    // its own, which has its own light: neither are its corona, its rays or the lamps below.)
    let dome = meshes.add(Sphere::new(1.).mesh().uv(48, 16));
    let room = bounds.room;
    if !room {
    commands.spawn((
        Mesh3d(dome.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: LinearRgba::rgb(look.disc.x, look.disc.y, look.disc.z).into(),
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::from_translation(sun + Vec3::Y * 0.012).with_scale(Vec3::new(SUN_RADIUS, 0.05, SUN_RADIUS)),
        NotShadowCaster,
        Surveyed,
        ThemeOwned,
    ));
    }

    let mut strand = |commands: &mut Commands, mesh: Filaments, material: FilamentMaterial| {
        if mesh.len() == 0 {
            return;
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh.mesh())),
            MeshMaterial3d(filaments.add(material)),
            Transform::IDENTITY,
            NoFrustumCulling,
            NotShadowCaster,
            ThemeOwned,
        ));
    };

    // A corona of fine radial spikes around the disc, with a soft halo behind them.
    let mut corona = filaments::corona(rays(1600, 800, 120), sun - Vec3::Y * 0.006, SUN_RADIUS, 0.75, 7);
    if quality != Quality::Minimal {
        corona.push(sun - Vec3::Y * 0.03, sun - Vec3::Y * 0.031, SUN_RADIUS * 5.5, 0.05, 0.5, filaments::Shape::Sprite);
    }
    if !room {
        strand(&mut commands, corona, filament_material(look.corona, [1.7, 0.45, 0., 0.], [0.05, 0.3, 0.85, 1.], 0.));
    }

    // The beam, as thousands of separate hair-thin rays from the disc to the counter. Temporal
    // anti-aliasing resolves a ray narrower than a pixel; without it a ray is drawn a full pixel wide.
    let thinnest = if quality == Quality::Full { 0.7 } else { 1. };
    let reach = Vec3::new(0.2, 0., 0.15);
    let low = Vec3::new(bounds.min.x, bounds.floor, bounds.min.y) - reach;
    let high = Vec3::new(bounds.max.x, bounds.floor, bounds.max.y) + reach;
    if theme == Theme::Jungle {
        // Under a canopy the light arrives as bundles of parallel shafts through the gaps in the leaves.
        let shafts = filaments::shafts(rays(5000, 2400, 400), low, high, bounds.ceiling - 0.45, sunlight, 9);
        strand(&mut commands, shafts, filament_material(look.rays, [1.3, 0.55, 0., 0.], [0.35, 1.3, 0.55, thinnest], 1.8));
    } else {
        // Seven sheaves across the counter, landing alternately toward the front and the back of it.
        let pools: Vec<(Vec3, f32)> = (0..7)
            .map(|i| {
                let across = bounds.min.x + size.x * (0.06 + 0.148 * i as f32);
                let deep = bounds.min.y + size.y * [0.30, 0.74, 0.50, 0.20, 0.82, 0.42, 0.66][i];
                (Vec3::new(across, bounds.floor, deep), 0.07 + 0.012 * ((i * 5) % 7) as f32)
            })
            .collect();
        let beam = filaments::sunbeam(rays(5000, 2400, 400), sun, SUN_RADIUS * 0.92, low, high, &pools, 9);
        if !room {
            strand(&mut commands, beam, filament_material(look.rays, [1.3, 0.55, 0., 0.], [0.35, 1.3, 0.7, thinnest], 1.8));
        }
    }

    // Glints: flecks of reflected light scattered over the counter, each twinkling on its own.
    let along = Vec2::new(sunlight.x, sunlight.z).normalize_or(Vec2::Y);
    let glints = filaments::glints(
        rays(2200, 1000, 0),
        Vec3::new(bounds.min.x, bounds.floor + 0.0012, bounds.min.y),
        Vec3::new(bounds.max.x, bounds.floor + 0.0012, bounds.max.y),
        along,
        11,
    );
    strand(&mut commands, glints, filament_material(look.glints, [2.6, 0.97, 0., 0.], [0.03, 0.14, 0., 1.], 0.));

    // Dust in the air, catching the light.
    let motes = filaments::motes(
        rays(3600, 1500, 0),
        Vec3::new(bounds.min.x - 0.5, bounds.floor + 0.01, bounds.min.y - 1.3),
        Vec3::new(bounds.max.x + 0.5, bounds.floor + 1.25, bounds.max.y),
        (0.0012, 0.0034),
        13,
    );
    strand(&mut commands, motes, filament_material(look.motes, [1.1, 0.75, 0.035, 0.32], [0.04, 0.2, 0., 1.2], 5.));

    // Many small lamps in place of a few large ones. Each is a bead of light with a little star of
    // spikes; only a handful also carry a real light, and none of those cast shadows.
    let mut beads: Vec<Vec3> = Vec::new();
    let mut lamps: Vec<Vec3> = Vec::new();
    let (bead_color, lamp_color, lumens, bead_size) = match theme {
        Theme::Kitchen => {
            // A ring of small bulbs around the sun, and a grid of pin lights across the ceiling.
            let ring = tier(32, 24, 12);
            for i in 0..ring {
                let turn = i as f32 / ring as f32 * TAU;
                beads.push(sun + Vec3::new(turn.cos(), -0.012, turn.sin()) * Vec3::new(0.78, 1., 0.78));
            }
            let (columns, rows) = (tier(8, 6, 4), 3);
            for row in 0..rows {
                for column in 0..columns {
                    let at = Vec3::new(
                        bounds.min.x + size.x * (column as f32 + 0.5) / columns as f32,
                        bounds.ceiling - 0.012,
                        bounds.min.y - 0.55 + (size.y + 0.4) * row as f32 / (rows - 1) as f32,
                    );
                    beads.push(at);
                    if (row + column) % 2 == 0 && row != 1 {
                        lamps.push(at - Vec3::Y * 0.05);
                    }
                }
            }
            (Vec3::new(9., 7.4, 4.6), Color::srgb(1.0, 0.90, 0.74), 600., 0.05)
        }
        Theme::Night => {
            // Strings of warm fairy lights: one sagging between hooks under the cabinets, one along
            // the front of the counter below its top, and a loose ring around the moon.
            let count = tier(40, 30, 14);
            for i in 0..count {
                let t = (i as f32 + 0.5) / count as f32;
                let sag = ((t * 6.).fract() - 0.5).powi(2) * 0.16;
                let at = Vec3::new(bounds.min.x + size.x * t, bounds.floor + 0.655 + sag, bounds.max.y - 0.30);
                beads.push(at);
                if i % 5 == 2 {
                    lamps.push(at - Vec3::Y * 0.03);
                }
                let sag = ((t * 5. + 0.5).fract() - 0.5).powi(2) * 0.12;
                let at = Vec3::new(bounds.min.x + size.x * t, bounds.floor - 0.085 + sag, bounds.min.y - 0.035);
                beads.push(at);
                if i % 8 == 4 {
                    lamps.push(at - Vec3::Z * 0.05);
                }
            }
            let ring = tier(18, 12, 6);
            for i in 0..ring {
                let turn = i as f32 / ring as f32 * TAU;
                beads.push(sun + Vec3::new(turn.cos(), -0.012, turn.sin()) * Vec3::new(0.60, 1., 0.60));
            }
            (Vec3::new(10., 4.6, 1.3), Color::srgb(1.0, 0.62, 0.30), 420., 0.045)
        }
        Theme::Jungle => {
            // Fireflies, drifting among the plants at either end of the counter and along the wall.
            let mut dice = Dice(31);
            for _ in 0..tier(70, 40, 12) {
                let side = dice.next();
                let at = if side < 0.4 {
                    Vec3::new(bounds.min.x - dice.range(0.05, 0.5), 0., dice.range(bounds.min.y - 0.3, bounds.max.y))
                } else if side < 0.8 {
                    Vec3::new(bounds.max.x + dice.range(0.05, 0.5), 0., dice.range(bounds.min.y - 0.3, bounds.max.y))
                } else {
                    Vec3::new(dice.range(bounds.min.x, bounds.max.x), 0., bounds.max.y - dice.range(0.0, 0.25))
                };
                beads.push(at + Vec3::Y * (bounds.floor + dice.range(0.02, 0.62)));
            }
            for i in 0..4 {
                let x = if i % 2 == 0 { bounds.min.x - 0.25 } else { bounds.max.x + 0.25 };
                lamps.push(Vec3::new(x, bounds.floor + 0.3, bounds.min.y + size.y * (0.25 + 0.5 * (i / 2) as f32)));
            }
            (Vec3::new(5., 7., 1.2), Color::srgb(0.75, 1.0, 0.35), 500., 0.03)
        }
    };
    let drifting = theme == Theme::Jungle;
    if room {
        (beads, lamps) = (vec![], vec![]);
    }
    let stars = filaments::sparkles(&beads, tier(10, 6, 0), bead_size * 1.6, bead_size, 17);
    strand(
        &mut commands,
        stars,
        filament_material(
            (bead_color * 0.16).extend(1.),
            if drifting { [0.9, 0.95, 0.05, 0.25] } else { [2.2, 0.3, 0., 0.] },
            [0.02, 0.1, 0.8, 1.2],
            0.,
        ),
    );
    if !drifting {
        let bulb = meshes.add(Sphere::new(0.006).mesh().uv(10, 6));
        let glass = materials.add(StandardMaterial {
            base_color: LinearRgba::rgb(bead_color.x, bead_color.y, bead_color.z).into(),
            unlit: true,
            fog_enabled: false,
            ..default()
        });
        for &at in &beads {
            commands.spawn((
                Mesh3d(bulb.clone()),
                MeshMaterial3d(glass.clone()),
                Transform::from_translation(at),
                NotShadowCaster,
                Surveyed,
                ThemeOwned,
            ));
        }
    }
    // Each lamp is lit in every pixel it reaches, so a lower detail keeps fewer: spread over the same
    // places, and each brighter by as much, so that the room keeps its fill light.
    let kept = detail.lamps.min(lamps.len());
    let boost = if kept > 0 { (lamps.len().min(12) as f32 / kept as f32).clamp(1., 3.) } else { 1. };
    for at in (0..kept).map(|i| lamps[i * lamps.len() / kept]) {
        commands.spawn((
            PointLight {
                color: lamp_color,
                intensity: 0.,
                range: if theme == Theme::Kitchen { 3. } else { 1.4 },
                radius: 0.01,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(at),
            IntroScaled(lumens * boost),
            ThemeOwned,
        ));
    }

    // Air for the light to show in: one volume of fog over the counter and the room in front of it.
    if quality == Quality::Full {
        commands.spawn((
            FogVolume {
                fog_color: look.haze.mix(&Color::WHITE, 0.5),
                density_factor: look.air,
                absorption: 0.2,
                scattering: 0.35,
                scattering_asymmetry: 0.55,
                light_tint: look.sun,
                light_intensity: 1.,
                ..default()
            },
            Transform::from_translation(Vec3::new(centre.x, bounds.ceiling / 2., centre.z - 1.2))
                .with_scale(Vec3::new(size.x + 2.4, bounds.ceiling + 0.2, size.y + 4.2)),
            ThemeOwned,
        ));
    }

    if theme == Theme::Jungle {
        foliage::spawn(
            &mut commands,
            &mut meshes,
            &mut materials,
            (quality != Quality::Minimal).then(|| library.as_ref().map(|l| &l.leaf)).flatten(),
            &bounds,
            foliage::Growth { plants: detail.plants, lean: quality == Quality::Minimal, canopy: shape.shadows },
            sunlight,
        );
    }
}

/// Marks a camera that has been given the look.
#[derive(Component)]
struct Dressed;

/// Gives every 3D camera the post-processing and fill light of the current theme and detail, and takes
/// away what the detail no longer has.
fn dress_cameras(
    mut commands: Commands,
    theme: Res<Theme>,
    quality: Res<Quality>,
    detail: Option<Res<Detail>>,
    mut stage: ResMut<Stage>,
    cameras: Query<(Entity, Has<Dressed>), (With<Camera3d>, Without<crate::viewmodel::ViewCamera>)>,
    views: Query<Entity, With<crate::viewmodel::ViewCamera>>,
) {
    let Some(detail) = detail else { return };
    if stage.applied.is_none() {
        return;
    }
    let look = theme.look();
    let restyle = stage.restyle;
    // A CPU renderer pays for every term in every pixel: the minimal set gets the theme's light and
    // nothing else.
    let minimal = *quality == Quality::Minimal;
    let full = *quality == Quality::Full;
    let hdr = detail.hdr && !minimal;
    let reflections = detail.reflections && !minimal && stage.environment.is_some();
    for (entity, dressed) in &cameras {
        if dressed && !restyle {
            continue;
        }
        let mut camera = commands.entity(entity);
        camera.insert((Dressed, AmbientLight { color: look.ambient, brightness: 0., ..default() }));
        if detail.fog && !minimal {
            camera.insert(DistanceFog {
                color: look.haze,
                directional_light_color: look.glow,
                directional_light_exponent: 12.,
                // (A room of its own is several metres across: its haze thinner, or its far side would be lost in it.)
                falloff: FogFalloff::Exponential { density: look.density * if stage.applied.is_some_and(|(_, bounds, _)| bounds.room) { 0.25 } else { 1. } },
            });
        } else {
            camera.remove::<DistanceFog>();
        }
        if minimal {
            camera.remove::<ColorGrading>();
        } else {
            let [exposure, temperature, saturation, contrast] = look.grade;
            camera.insert(ColorGrading::with_identical_sections(
                ColorGradingGlobal { exposure, temperature, post_saturation: saturation, ..default() },
                ColorGradingSection { contrast, ..default() },
            ));
        }
        // Without HDR the picture is tone mapped in each shader, and there is nothing to bloom.
        if hdr {
            camera.insert((Hdr, Bloom { intensity: look.bloom, ..Bloom::NATURAL }));
        } else {
            camera.remove::<(Bloom, Hdr)>();
        }
        match (&stage.environment, reflections) {
            (Some(environment), true) => {
                camera.insert(EnvironmentMapLight {
                    diffuse_map: environment.diffuse.clone(),
                    specular_map: environment.specular.clone(),
                    intensity: 0.,
                    ..default()
                });
            }
            _ => {
                camera.remove::<EnvironmentMapLight>();
            }
        }
        if !full {
            // What only the full set has, with what temporal anti-aliasing and occlusion brought along.
            camera.remove::<(
                TemporalAntiAliasing,
                TemporalJitter,
                MipBias,
                ContrastAdaptiveSharpening,
                ContactShadows,
                VolumetricFog,
                Vignette,
                ChromaticAberration,
                ScreenSpaceAmbientOcclusion,
            )>();
            camera.remove::<(DepthPrepass, NormalPrepass, MotionVectorPrepass)>();
            // Without multisampling `viewmodel::stack` gives the finished picture FXAA. At Low and Lowest, with a lamp
            // or two at most, one cluster holds every light: the same light everywhere, without sorting each lamp into
            // thousands of clusters on the main thread every frame.
            camera.insert((
                if detail.msaa { Msaa::Sample4 } else { Msaa::Off },
                if minimal || !detail.soft_shadows { ShadowFilteringMethod::Hardware2x2 } else { ShadowFilteringMethod::Gaussian },
                if detail.level >= crate::quality::Level::Low { ClusterConfig::Single } else { ClusterConfig::default() },
            ));
            continue;
        }
        camera.insert((
            // Temporal anti-aliasing replaces MSAA and lets shadows, occlusion and fog be sampled
            // with noise that it then averages away.
            Msaa::Off,
            TemporalAntiAliasing::default(),
            ContrastAdaptiveSharpening { enabled: true, sharpening_strength: 0.35, denoise: false },
            ShadowFilteringMethod::Temporal,
            ContactShadows { linear_steps: 12, thickness: 0.012, length: 0.035 },
            VolumetricFog {
                ambient_color: look.haze,
                ambient_intensity: look.air_ambient,
                jitter: 0.5,
                step_count: 20,
            },
            Vignette { intensity: look.vignette, radius: 0.72, smoothness: 4., ..default() },
            ChromaticAberration { intensity: 0.006, max_samples: 4, ..default() },
        ));
        if stage.occlusion {
            camera.insert(ScreenSpaceAmbientOcclusion { constant_object_thickness: 0.03, ..default() });
        }
    }
    // `viewmodel::stack` moves the finishing steps onto the view camera and copies the reflections
    // there; it never takes them away, so what the detail dropped goes from there too.
    if restyle {
        for view in &views {
            let mut view = commands.entity(view);
            if !hdr {
                view.remove::<Bloom>();
            }
            if !full {
                view.remove::<(Vignette, ChromaticAberration, ContrastAdaptiveSharpening)>();
            }
            if !reflections {
                view.remove::<EnvironmentMapLight>();
            }
            if minimal {
                view.remove::<ColorGrading>();
            }
        }
    }
    stage.restyle = false;
}

/// How a picture drawn at `detail`'s scale is finished before the page stretches it to the screen
/// (`quality`): the strength of the sharpening ([`sharpen`]), which restores the edges the stretch
/// blurs (more the further it is stretched), and the mip bias that samples textures as finely as the
/// screen's own resolution would. None at full scale, or where the level does not sharpen.
pub fn sharpening(detail: &Detail) -> Option<(f32, f32)> {
    if !detail.sharpen || detail.scale >= 0.99 {
        return None;
    }
    let strength = ((1. - detail.scale) * 2.).clamp(0.2, 0.9);
    let bias = detail.scale.max(0.1).log2().clamp(-1., 0.);
    Some((strength, bias))
}

/// Puts [`sharpening`] on the cameras: the sharpening on the view camera, which finishes the picture
/// (`viewmodel::stack`), and the mip bias on every 3D camera (`try_`: the death camera and others come and go
/// within a frame). The full set leaves them to temporal
/// anti-aliasing and Bevy's own sharpening (`dress_cameras`), which WebGL 2 cannot run (`sharpen`).
fn sharpen(
    mut commands: Commands,
    detail: Option<Res<Detail>>,
    quality: Res<Quality>,
    options: Res<VisualOptions>,
    views: Query<(Entity, Option<&sharpen::Sharpen>), With<crate::viewmodel::ViewCamera>>,
    cameras: Query<(Entity, Option<&MipBias>), With<Camera3d>>,
) {
    let Some(detail) = detail else { return };
    let wanted = sharpening(&detail)
        .filter(|_| *quality != Quality::Full)
        .map(|(strength, bias)| (options.sharpness.unwrap_or(strength), bias));
    for (view, now) in &views {
        match (wanted.filter(|(strength, _)| *strength > 0.), now) {
            (Some((strength, _)), Some(now)) if (now.strength - strength).abs() < 0.001 => {}
            (Some((strength, _)), _) => {
                commands.entity(view).try_insert(sharpen::Sharpen { strength });
            }
            (None, Some(_)) => {
                commands.entity(view).try_remove::<sharpen::Sharpen>();
            }
            (None, None) => {}
        }
    }
    if *quality == Quality::Full {
        return;
    }
    for (camera, now) in &cameras {
        match (wanted, now) {
            (Some((_, bias)), Some(now)) if (now.0 - bias).abs() < 0.001 => {}
            (Some((_, bias)), _) => {
                commands.entity(camera).try_insert(MipBias(bias));
            }
            (None, Some(_)) => {
                commands.entity(camera).try_remove::<MipBias>();
            }
            (None, None) => {}
        }
    }
}

/// The kitchen's own lamps keep their shadows only where the detail allows: each is a shadow pass of
/// its own (six, for a point light), and once the round starts the theme has turned them down to a
/// glimmer. A CPU renderer has none (`runtime::software_lighting`).
#[derive(Component)]
struct CastsShadows(bool);

fn lamp_shadows(
    mut commands: Commands,
    detail: Option<Res<Detail>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut spots: Query<(Entity, &mut SpotLight, Option<&CastsShadows>), Without<ThemeOwned>>,
    mut points: Query<(Entity, &mut PointLight, Option<&CastsShadows>), (Without<ThemeOwned>, Without<SpotLight>)>,
) {
    let Some(detail) = detail else { return };
    let software = adapter.is_some_and(|a| format!("{:?}", a.device_type) == "Cpu");
    let allowed = detail.lamp_shadows && !software;
    let mut follow = |entity: Entity, enabled: &mut bool, own: Option<&CastsShadows>| {
        let own = own.map_or_else(
            || {
                commands.entity(entity).try_insert(CastsShadows(*enabled));
                *enabled
            },
            |o| o.0,
        );
        if *enabled != (own && allowed) {
            *enabled = own && allowed;
        }
    };
    for (entity, mut spot, own) in &mut spots {
        if spot.shadow_maps_enabled != (own.is_none_or(|o| o.0) && allowed) || own.is_none() {
            follow(entity, &mut spot.shadow_maps_enabled, own);
        }
    }
    for (entity, mut point, own) in &mut points {
        if point.shadow_maps_enabled != (own.is_none_or(|o| o.0) && allowed) || own.is_none() {
            follow(entity, &mut point.shadow_maps_enabled, own);
        }
    }
}

/// What a lamp of the kitchen's own was set to before any theme touched it.
#[derive(Component)]
struct Baseline(f32);

/// Turns the kitchen's own lamps down to what the theme leaves of them.
fn tune_room_lights(
    mut commands: Commands,
    theme: Res<Theme>,
    game: Option<Res<crate::arena::Game>>,
    detail: Option<Res<Detail>>,
    // (The kitchen floor's moon and fridge keep their own brightness: floor.rs.)
    mut spots: Query<(Entity, &mut SpotLight, Option<&Baseline>, Option<&mut Visibility>, Has<crate::fridge::RoomLamp>), (Without<ThemeOwned>, Without<crate::floor::Lamp>)>,
    mut points: Query<
        (Entity, &mut PointLight, Option<&Baseline>, Option<&mut Visibility>, Has<crate::fridge::RoomLamp>),
        (
            Without<ThemeOwned>,
            Without<SpotLight>,
            Without<crate::fridge::FridgeLamp>,
            Without<crate::maya::TaskLamp>,
            // A stimmed hotdog's glow keeps the brightness its stim gives it (player_model/stim.rs).
            Without<crate::player_model::stim::StimLight>,
            Without<crate::floor::Lamp>,
        ),
    >,
) {
    let look = theme.look();
    // Until the round starts the ceiling spot lights the breakfast scene; then the sun takes over.
    let playing = game.is_some_and(|g| g.playing());
    let room = if playing { look.room } else { look.room.sqrt() };
    // At the Lowest level the kitchen's own lamps go out in a round (`Detail::room_lamps`): turned down to a glimmer
    // there, they cost every pixel they reach a light of its own. Hidden, they are left out of the lights altogether.
    let shown = if !playing || detail.is_none_or(|d| d.room_lamps) { Visibility::Inherited } else { Visibility::Hidden };
    let show = |commands: &mut Commands, entity: Entity, visibility: Option<Mut<Visibility>>| match visibility {
        Some(mut v) if *v != shown && (*v == Visibility::Hidden || *v == Visibility::Inherited) => *v = shown,
        None if shown == Visibility::Hidden => {
            commands.entity(entity).try_insert(Visibility::Hidden);
        }
        _ => {}
    };
    for (entity, mut spot, baseline, visibility, ours) in &mut spots {
        let full = baseline.map_or(spot.intensity, |b| b.0);
        if baseline.is_none() {
            commands.entity(entity).try_insert(Baseline(full));
        }
        if spot.intensity != full * room {
            spot.intensity = full * room;
        }
        if ours {
            show(&mut commands, entity, visibility);
        }
    }
    for (entity, mut point, baseline, visibility, ours) in &mut points {
        let full = baseline.map_or(point.intensity, |b| b.0);
        if baseline.is_none() {
            commands.entity(entity).try_insert(Baseline(full));
        }
        if point.intensity != full * look.fill {
            point.intensity = full * look.fill;
        }
        if ours {
            show(&mut commands, entity, visibility);
        }
    }
}

/// How many times the theme's flat fill a room of its own gets: drawn in high dynamic range with the full feature set
/// (High: its occlusion and marched fog take more of it back), in high dynamic range with the WebGL 2 set (Medium), without
/// it but with the reflected room (Low), and with neither (Lowest); and its colour there. Set by looking at
/// the room at each level, natively and in a browser.
pub const ROOM_AMBIENT: [f32; 4] = [12., 7., 2.5, 5.];
pub const ROOM_TINT: Color = Color::srgb(0.42, 0.5, 0.82);

/// Raises the theme's light as the refrigerator door opens in the intro, and holds it at full once the
/// round is on. Before that the camera is shut in the fridge, which the theme must leave dark.
fn follow_intro(
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    game: Option<Res<crate::arena::Game>>,
    scene: Option<Res<crate::fridge::FridgeScene>>,
    mut suns: Query<(&IntroScaled, &mut DirectionalLight)>,
    mut lamps: Query<(&IntroScaled, &mut PointLight)>,
    mut cameras: Query<(&mut Camera, &mut AmbientLight, Option<&mut EnvironmentMapLight>), With<Dressed>>,
    bounds: Option<Res<ArenaBounds>>,
    detail: Option<Res<Detail>>,
) {
    let open = if game.is_some_and(|g| g.playing()) {
        1.
    } else if let Some(scene) = scene {
        scene
            .elapsed(time.elapsed_secs_f64())
            .map_or(0., |elapsed| ((elapsed - 2.05) / 0.18).clamp(0., 1.))
    } else {
        1.
    };
    let look = theme.look();
    for (full, mut sun) in &mut suns {
        if sun.illuminance != full.0 * open {
            sun.illuminance = full.0 * open;
        }
    }
    for (full, mut lamp) in &mut lamps {
        if lamp.intensity != full.0 * open {
            lamp.intensity = full.0 * open;
        }
    }
    let beyond = look.haze.to_linear() * open;
    // A room of its own (`ArenaBounds::room`, floor.rs) is shut in by its walls and ceiling: what the theme's sky would
    // bounce into it is raised (`ROOM_AMBIENT`), so that walls, cabinets and furniture stay dark blue against darker at
    // every level rather than black on black. It costs nothing per light.
    let room = bounds.is_some_and(|b| b.room);
    let (hdr, full) = detail.as_ref().map_or((true, true), |d| (d.hdr, d.tier == Quality::Full));
    for (mut camera, mut ambient, environment) in &mut cameras {
        let lift = match (room, hdr, environment.is_some()) {
            (false, ..) => 1.,
            (true, true, _) if full => ROOM_AMBIENT[0],
            (true, true, _) => ROOM_AMBIENT[1],
            (true, false, true) => ROOM_AMBIENT[2],
            (true, false, false) => ROOM_AMBIENT[3],
        };
        // (Moonlight thrown about a room is greyer than the night sky's blue.)
        let tint = if room { ROOM_TINT } else { look.ambient };
        if ambient.color != tint {
            ambient.color = tint;
        }
        let nits = look.nits * open * lift;
        if ambient.brightness != nits {
            ambient.brightness = nits;
        }
        if let Some(mut environment) = environment {
            if environment.intensity != look.environment * open {
                environment.intensity = look.environment * open;
            }
        }
        let clear = ClearColorConfig::Custom(beyond.into());
        if !matches!((&camera.clear_color, &clear), (ClearColorConfig::Custom(a), ClearColorConfig::Custom(b)) if a == b) {
            camera.clear_color = clear;
        }
    }
}

/// Marching through the fog is paid for per pixel and per step, and is the one effect here whose cost
/// a large window multiplies: about 1.5 ms per step at 3840 x 2160 on an Arc B580. Fewer steps on more
/// pixels keeps it to a few milliseconds at any size; temporal anti-aliasing smooths what coarser
/// steps leave.
fn pace_volumetrics(windows: Query<&Window>, mut fogs: Query<&mut VolumetricFog>) {
    let Some(pixels) = windows.iter().map(|w| w.physical_width() * w.physical_height()).max() else {
        return;
    };
    let steps = (20. * 921_600. / pixels.max(1) as f32).clamp(6., 20.) as u32;
    for mut fog in &mut fogs {
        if fog.step_count != steps {
            fog.step_count = steps;
        }
    }
}

/// `--photo x,y,z,tx,ty,tz[,fov]` / `?photo=...`: holds the camera on a fixed view, for screenshots.
fn photo_camera(
    options: Res<VisualOptions>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let Some(shot) = options.photo else {
        return;
    };
    for (mut transform, mut projection) in &mut cameras {
        *transform = Transform::from_xyz(shot[0], shot[1], shot[2])
            .looking_at(Vec3::new(shot[3], shot[4], shot[5]), Vec3::Y);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = shot[6].to_radians();
        }
    }
}

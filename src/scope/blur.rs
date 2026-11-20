// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The blur round the syrup rifle's optic on the levels that have it (scope.rs `path`): [`ScopeBlur`] on the view camera,
//! which finishes the picture (viewmodel.rs), runs `blur.wgsl` over it twice, across and then down, after the bloom and
//! before the tone mapping, so that the head-up display, drawn after, stays sharp. Inside the optic's ring the picture
//! is left as it is; outside it the blur comes up over a band and is then at its fullest, the same everywhere. Each
//! pass reads seventeen taps a pixel along its way (one inside the ring, where it is cut short), so the two cost about
//! as much as a few fullscreen copies; and only while the optic is up. WebGL 2 runs it as it is: one uniform of
//! thirty-two bytes, two fullscreen draws.
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        schedule::{Core3d, Core3dSystems},
        tonemapping::tonemapping,
        FullscreenShader,
    },
    ecs::query::QueryItem,
    post_process::bloom::bloom,
    prelude::*,
    render::{
        extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin},
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        diagnostic::RecordDiagnostics,
        renderer::{RenderContext, RenderDevice, ViewQuery},
        sync_component::SyncComponent,
        view::{ExtractedView, ViewTarget},
        Render, RenderApp, RenderStartup, RenderSystems,
    },
};

/// Blurs a camera's picture outside a circle: sharp within `radius` of `centre`, blurred to `size` across `feather`
/// beyond it. `centre` is a place on the picture (0 to 1 across and down); the rest are shares of its height.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct ScopeBlur {
    pub centre: Vec2,
    pub radius: f32,
    pub feather: f32,
    pub size: f32,
}

/// What the shader is given: the circle (centre, radius, feather) and the size; thirty-two bytes.
#[derive(Component, ShaderType, Clone)]
pub struct BlurUniform {
    circle: Vec4,
    size: Vec4,
}

impl SyncComponent for ScopeBlur {
    type Target = BlurUniform;
}

impl ExtractComponent for ScopeBlur {
    type QueryData = &'static Self;
    type QueryFilter = With<Camera>;
    type Out = BlurUniform;

    fn extract_component(item: QueryItem<Self::QueryData>) -> Option<Self::Out> {
        (item.size > 0.).then(|| BlurUniform { circle: Vec4::new(item.centre.x, item.centre.y, item.radius, item.feather.max(1e-4)), size: Vec4::new(item.size, 0., 0., 0.) })
    }
}

pub struct Plugin;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "blur.wgsl");
        app.add_plugins((ExtractComponentPlugin::<ScopeBlur>::default(), UniformComponentPlugin::<BlurUniform>::default()));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(Core3d, blur.after(bloom).before(tonemapping).in_set(Core3dSystems::PostProcess));
    }
}

#[derive(Resource)]
struct BlurPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    variants: Variants<RenderPipeline, Specialize>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>, server: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "scope_blur_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer::<BlurUniform>(true)),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    let variants = Variants::new(
        Specialize,
        RenderPipelineDescriptor {
            label: Some("scope_blur".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState { shader: load_embedded_asset!(server.as_ref(), "blur.wgsl"), ..default() }),
            ..default()
        },
    );
    commands.insert_resource(BlurPipeline { layout, sampler, variants });
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, SpecializerKey)]
struct Key {
    format: TextureFormat,
    /// The second pass, down the picture; the first goes across it.
    down: bool,
}

struct Specialize;

impl Specializer<RenderPipeline> for Specialize {
    type Key = Key;

    fn specialize(&self, key: Key, descriptor: &mut <RenderPipeline as Specializable>::Descriptor) -> Result<Canonical<Key>, BevyError> {
        let fragment = descriptor.fragment_mut()?;
        fragment.set_target(0, ColorTargetState { format: key.format, blend: None, write_mask: ColorWrites::ALL });
        if key.down {
            fragment.shader_defs.push("DOWN".into());
        }
        Ok(key)
    }
}

/// The two passes' pipelines for a view, and the picture's format they were made for.
#[derive(Component)]
struct ViewBlurPipelines {
    across: CachedRenderPipelineId,
    down: CachedRenderPipelineId,
    format: TextureFormat,
}

fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlurPipeline>,
    views: Query<(Entity, &ExtractedView, Option<&ViewBlurPipelines>), With<BlurUniform>>,
    mut removed: RemovedComponents<BlurUniform>,
) -> Result<(), BevyError> {
    for entity in removed.read() {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<ViewBlurPipelines>();
        }
    }
    for (entity, view, made) in &views {
        if made.is_some_and(|m| m.format == view.target_format) {
            continue;
        }
        let across = pipeline.variants.specialize(&cache, Key { format: view.target_format, down: false })?;
        let down = pipeline.variants.specialize(&cache, Key { format: view.target_format, down: true })?;
        commands.entity(entity).insert(ViewBlurPipelines { across, down, format: view.target_format });
    }
    Ok(())
}

fn blur(
    view: ViewQuery<(&ViewTarget, &ViewBlurPipelines, &DynamicUniformIndex<BlurUniform>), With<ExtractedView>>,
    pipeline: Res<BlurPipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<BlurUniform>>,
    mut ctx: RenderContext,
    mut cached: Local<Vec<(BufferId, TextureViewId, BindGroup)>>,
) {
    let (target, made, index) = view.into_inner();
    let (Some(buffer), Some(binding)) = (uniforms.buffer(), uniforms.binding()) else {
        return;
    };
    let (Some(across), Some(down)) = (cache.get_render_pipeline(made.across), cache.get_render_pipeline(made.down)) else {
        return;
    };
    // (Timed as Bevy's own passes are, for the render diagnostics: `render/scope_blur/elapsed_gpu`.)
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "scope_blur");
    for render_pipeline in [across, down] {
        // Each pass reads the picture as the last left it and writes the other of the view's two.
        let post = target.post_process_write();
        let found = cached.iter().find(|(b, t, _)| *b == buffer.id() && *t == post.source.id()).map(|(_, _, g)| g.clone());
        let bind_group = match found {
            Some(group) => group,
            None => {
                let group = ctx.render_device().create_bind_group(
                    "scope_blur_bind_group",
                    &cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((post.source, &pipeline.sampler, binding.clone())),
                );
                cached.retain(|(b, _, _)| *b == buffer.id());
                if cached.len() >= 4 {
                    cached.remove(0);
                }
                cached.push((buffer.id(), post.source.id(), group.clone()));
                group
            }
        };
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("scope_blur"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(render_pipeline);
        pass.set_bind_group(0, &bind_group, &[index.index()]);
        pass.draw(0..3, 0..1);
    }
    span.end(ctx.command_encoder());
}

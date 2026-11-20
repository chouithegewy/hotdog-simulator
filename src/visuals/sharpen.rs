// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Sharpening that works in WebGL 2: [`Sharpen`] on a camera runs `sharpen.wgsl` over its finished picture, after FXAA
//! and before the head-up display, at the resolution the picture is drawn at, just before the page stretches it to the
//! screen (`quality`'s render scale).
//!
//! Bevy 0.19 has contrast-adaptive sharpening (`ContrastAdaptiveSharpening`), but its uniform is four bytes, and WebGL 2
//! binds uniform buffers only in multiples of sixteen: its pipeline fails validation there and the game quits. This is
//! the same kind of pass with a sixteen-byte uniform, as one fullscreen draw of five taps a pixel.
use bevy::{
    anti_alias::{contrast_adaptive_sharpening::cas, fxaa::fxaa},
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        schedule::{Core3d, Core3dSystems},
        FullscreenShader,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        sync_component::SyncComponent,
        view::{ExtractedView, ViewTarget},
        Render, RenderApp, RenderStartup, RenderSystems,
    },
};

/// Sharpens the camera's finished picture: `strength` from 0 (nothing) to 1.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Sharpen {
    pub strength: f32,
}

/// What the shader is given: the strength in `x`; sixteen bytes, as WebGL 2 wants.
#[derive(Component, ShaderType, Clone)]
pub struct SharpenUniform {
    settings: Vec4,
}

impl SyncComponent for Sharpen {
    type Target = SharpenUniform;
}

impl ExtractComponent for Sharpen {
    type QueryData = &'static Self;
    type QueryFilter = With<Camera>;
    type Out = SharpenUniform;

    fn extract_component(item: QueryItem<Self::QueryData>) -> Option<Self::Out> {
        (item.strength > 0.).then(|| SharpenUniform { settings: Vec4::new(item.strength.clamp(0., 1.), 0., 0., 0.) })
    }
}

pub struct SharpenPlugin;

impl Plugin for SharpenPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "sharpen.wgsl");
        app.add_plugins((ExtractComponentPlugin::<Sharpen>::default(), UniformComponentPlugin::<SharpenUniform>::default()));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(Core3d, sharpen.after(fxaa).after(cas).in_set(Core3dSystems::PostProcess));
    }
}

#[derive(Resource)]
struct SharpenPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    variants: Variants<RenderPipeline, Specialize>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>, server: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "sharpen_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer::<SharpenUniform>(true)),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor::default());
    let variants = Variants::new(
        Specialize,
        RenderPipelineDescriptor {
            label: Some("sharpen".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState { shader: load_embedded_asset!(server.as_ref(), "sharpen.wgsl"), ..default() }),
            ..default()
        },
    );
    commands.insert_resource(SharpenPipeline { layout, sampler, variants });
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, SpecializerKey)]
struct Key {
    format: TextureFormat,
}

struct Specialize;

impl Specializer<RenderPipeline> for Specialize {
    type Key = Key;

    fn specialize(&self, key: Key, descriptor: &mut <RenderPipeline as Specializable>::Descriptor) -> Result<Canonical<Key>, BevyError> {
        descriptor.fragment_mut()?.set_target(0, ColorTargetState { format: key.format, blend: None, write_mask: ColorWrites::ALL });
        Ok(key)
    }
}

#[derive(Component)]
struct ViewSharpenPipeline(CachedRenderPipelineId, TextureFormat);

fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<SharpenPipeline>,
    views: Query<(Entity, &ExtractedView, Option<&ViewSharpenPipeline>), With<SharpenUniform>>,
    mut removed: RemovedComponents<SharpenUniform>,
) -> Result<(), BevyError> {
    // (`try_`: a view's render entity may be gone by the time the commands are applied.)
    for entity in removed.read() {
        commands.entity(entity).try_remove::<ViewSharpenPipeline>();
    }
    for (entity, view, made) in &views {
        if made.is_some_and(|m| m.1 == view.target_format) {
            continue;
        }
        let id = pipeline.variants.specialize(&cache, Key { format: view.target_format })?;
        commands.entity(entity).try_insert(ViewSharpenPipeline(id, view.target_format));
    }
    Ok(())
}

fn sharpen(
    view: ViewQuery<(&ViewTarget, &ViewSharpenPipeline, &DynamicUniformIndex<SharpenUniform>), With<ExtractedView>>,
    pipeline: Res<SharpenPipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<SharpenUniform>>,
    mut ctx: RenderContext,
    mut cached: Local<Option<(BufferId, TextureViewId, BindGroup)>>,
) {
    let (target, made, index) = view.into_inner();
    let (Some(buffer), Some(binding)) = (uniforms.buffer(), uniforms.binding()) else {
        return;
    };
    let Some(render_pipeline) = cache.get_render_pipeline(made.0) else {
        return;
    };
    let post = target.post_process_write();
    let bind_group = match &mut *cached {
        Some((buffer_id, texture_id, bind_group)) if *buffer_id == buffer.id() && *texture_id == post.source.id() => bind_group,
        cached => {
            let bind_group = ctx.render_device().create_bind_group(
                "sharpen_bind_group",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((post.source, &pipeline.sampler, binding)),
            );
            &cached.insert((buffer.id(), post.source.id(), bind_group)).2
        }
    };
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("sharpen"),
        color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, bind_group, &[index.index()]);
    pass.draw(0..3, 0..1);
}

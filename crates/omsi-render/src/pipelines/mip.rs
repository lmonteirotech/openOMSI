//! Mip maps made on the GPU.

use super::common::*;

pub(crate) struct Mip {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
}

pub(crate) fn build(device: &wgpu::Device) -> Mip {
    // --- mipmaps on the GPU: the CPU box filter took up to a second per bus spawn
    log::info!("renderer: compiling the mip maps shaders");
    let mip_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mip"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../mip.wgsl").into()),
    });
    let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mip"),
        entries: &[float_texture_entry(0), sampler_entry(1)],
    });
    let mip_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mip"),
        bind_group_layouts: &[Some(&mip_layout)],
        immediate_size: 0,
    });
    let mip_pipeline = no_vertex_pipeline("mip", &mip_pl, &mip_shader, "fs_main", &[target(wgpu::TextureFormat::Rgba8UnormSrgb, None)]).create(device);
    let mip_sampler = clamped_linear_sampler(device);
    Mip { pipeline: mip_pipeline, layout: mip_layout, sampler: mip_sampler }
}

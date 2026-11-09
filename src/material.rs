//! The one material: an unlit, premultiplied-alpha MTSDF shader whose vertex
//! stage implements every [`crate::MsdfTextMode`].

use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

pub(crate) const SHADER: &str = "embedded://bevy_msdf_text/msdf_text.wgsl";

/// Per-text uniforms. Lengths are em unless noted.
#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct MsdfParams {
    pub fill: Vec4,
    pub outline: Vec4,
    pub shadow: Vec4,
    /// `Screen` only: xy = viewport anchor (0..1, top-left origin), zw = pixel offset.
    pub screen: Vec4,
    /// 0 World, 1 WorldFixedPx, 2 Screen, 3 Plane.
    pub mode: u32,
    /// World units per em (World, Plane) or physical pixels per em (WorldFixedPx, Screen).
    pub size: f32,
    pub outline_width: f32,
    pub shadow_softness: f32,
    /// The font's distance range in em and in atlas texels.
    pub range_em: f32,
    pub range_px: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct MsdfMaterial {
    #[uniform(0)]
    pub params: MsdfParams,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}

impl Material for MsdfMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// Screen text sorts after everything else in the transparent pass.
    fn depth_bias(&self) -> f32 {
        if self.params.mode == 2 { 1.0e6 } else { 0.0 }
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?];
        // Billboards may face away after their transform's rotation.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

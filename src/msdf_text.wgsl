// MTSDF text: RGB holds a multi-channel SDF (sharp corners), A a true SDF
// (rounded, correct far from the outline: outlines, shadows, glow).
//
// Vertex positions are em, +y up, relative to the text's anchor. The vertex
// stage turns them into clip space per mode; z is the layer: 0 fill,
// 1 outline, 2 shadow, drawn back to front as separate passes over the text.

#import bevy_pbr::{mesh_functions, mesh_view_bindings::view}

struct MsdfParams {
    fill: vec4<f32>,
    outline: vec4<f32>,
    shadow: vec4<f32>,
    screen: vec4<f32>,
    mode: u32,
    size: f32,
    outline_width: f32,
    shadow_softness: f32,
    range_em: f32,
    range_px: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: MsdfParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler: sampler;

const WORLD: u32 = 0u;
const WORLD_FIXED_PX: u32 = 1u;
const SCREEN: u32 = 2u;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Constant per quad, so plain interpolation keeps it exact.
    @location(1) layer: f32,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let anchor = world_from_local * vec4(0.0, 0.0, 0.0, 1.0);
    let em = v.position.xy * params.size;
    // Pixels -> NDC.
    let px = 2.0 / view.viewport.zw;

    var clip: vec4<f32>;
    switch params.mode {
        case WORLD: {
            // Billboard: the camera's right/up, scaled like any world object.
            let scale = length(world_from_local[0].xyz);
            let right = view.world_from_view[0].xyz;
            let up = view.world_from_view[1].xyz;
            clip = view.clip_from_world * vec4(anchor.xyz + (right * em.x + up * em.y) * scale, 1.0);
        }
        case WORLD_FIXED_PX: {
            // Project the anchor, then offset in pixels (scaled by w so the
            // perspective divide leaves them constant on screen).
            clip = view.clip_from_world * anchor;
            clip = vec4(clip.xy + em * px * clip.w, clip.zw);
            if clip.w <= 0.0 {
                clip = vec4(0.0, 0.0, 2.0, 1.0); // behind the camera: clipped
            }
        }
        case SCREEN: {
            let at = vec2(params.screen.x * 2.0 - 1.0, 1.0 - params.screen.y * 2.0);
            let offset = vec2(params.screen.z, -params.screen.w);
            // Reverse-Z: depth 1 is the near plane, in front of everything.
            clip = vec4(at + (offset + em) * px, 1.0, 1.0);
        }
        default: {
            // Plane: a flat object, oriented by its transform.
            clip = view.clip_from_world * (world_from_local * vec4(em, 0.0, 1.0));
        }
    }

    var out: VertexOutput;
    out.clip = clip;
    out.uv = v.uv;
    out.layer = v.position.z;
    return out;
}

fn median(r: f32, g: f32, b: f32) -> f32 {
    return max(min(r, g), min(max(r, g), b));
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4(c.rgb * c.a, c.a);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(atlas, atlas_sampler, in.uv);

    // msdfgen's screenPxRange: how many screen pixels the distance range spans
    // here. Derived from the uv derivatives, so it holds for any scale or tilt.
    let unit_range = vec2(params.range_px) / vec2<f32>(textureDimensions(atlas, 0));
    let screen_tex_size = 1.0 / fwidth(in.uv);
    let screen_px_range = max(0.5 * dot(unit_range, screen_tex_size), 1.0);
    let px_per_em = screen_px_range / params.range_em;

    // Signed distances in em, positive inside.
    let fill_d = (median(s.r, s.g, s.b) - 0.5) * params.range_em;
    let true_d = (s.a - 0.5) * params.range_em;
    let outline_d = true_d + params.outline_width;

    if in.layer > 1.5 {
        // Shadow / glow: the outlined shape, fading out over `softness`.
        let soft = max(params.shadow_softness, 1.0 / px_per_em);
        return premul(params.shadow) * smoothstep(-soft, 0.0, outline_d);
    }
    if in.layer > 0.5 {
        return premul(params.outline) * clamp(outline_d * px_per_em + 0.5, 0.0, 1.0);
    }
    return premul(params.fill) * clamp(fill_d * px_per_em + 0.5, 0.0, 1.0);
}

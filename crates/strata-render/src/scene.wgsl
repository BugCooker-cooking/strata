struct TileView { placement:vec4<f32>, uv:vec4<f32>, exposure:vec4<f32> };
struct Camera { geometry:vec4<f32>, scale:vec4<f32> };
@group(0) @binding(0) var<uniform> view:TileView;
@group(0) @binding(1) var source:texture_2d<f32>;
@group(0) @binding(2) var source_sampler:sampler;
@group(0) @binding(3) var coverage:texture_2d<f32>;
@group(1) @binding(0) var<uniform> camera:Camera;
struct VertexOutput { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index:u32)->VertexOutput {
    let p=array<vec2<f32>,6>(vec2(0,0),vec2(0,1),vec2(1,0),vec2(1,0),vec2(0,1),vec2(1,1));
    let world=view.placement.xy+p[index]*view.placement.z;
    let offset=(world-camera.geometry.xy)*camera.scale.x;
    var out:VertexOutput;
    out.position=vec4(offset.x*2.0/camera.geometry.z,-offset.y*2.0/camera.geometry.w,0,1);
    out.uv=view.uv.xy+p[index]*view.uv.zw;return out;
}
@fragment fn fs_main(input:VertexOutput)->@location(0) vec4<f32> {
    let base=textureSample(source,source_sampler,input.uv);
    let amount=textureSample(coverage,source_sampler,input.uv).r;
    return vec4(base.rgb*(1.0+view.exposure.x*amount),base.a);
}

struct View { image_to_ndc: mat4x4<f32> };
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 6>(
        vec2(0.0,0.0), vec2(0.0,1.0), vec2(1.0,0.0),
        vec2(1.0,0.0), vec2(0.0,1.0), vec2(1.0,1.0)
    );
    var output: VertexOutput;
    output.uv = positions[index];
    output.position = view.image_to_ndc * vec4(output.uv,0.0,1.0);
    return output;
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Input is already linear P3 with premultiplied alpha, BEFORE filtering.
    return textureSample(source, source_sampler, input.uv);
}

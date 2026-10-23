struct Appearance {
    positions: array<vec4<f32>, 384>,
    colors: array<vec4<f32>, 384>,
};
@group(0) @binding(0) var<uniform> appearance: Appearance;
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
};
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var output: VertexOutput;
    output.position = appearance.positions[index];
    output.color = appearance.colors[index].rgb;
    return output;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}

#version 450
layout(push_constant) uniform Appearance {
    vec4 positions[3];
    vec4 colors[3];
} appearance;
layout(location = 0) out vec3 color;
void main() {
    gl_Position = appearance.positions[gl_VertexIndex];
    gl_Position.y = -gl_Position.y;
    color = appearance.colors[gl_VertexIndex].rgb;
}

#version 330 core
uniform vec4 positions[384];
uniform vec4 colors[384];
out vec3 color;
void main() {
    gl_Position = positions[gl_VertexID];
    color = colors[gl_VertexID].rgb;
}

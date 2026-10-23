#version 330 core
in vec3 color;
out vec4 output_color;
void main() { output_color = vec4(color, 1.0); }

#version 300 es
precision highp float;
in vec3 vertex_color;
out vec4 output_color;
void main(){output_color=vec4(vertex_color,1.0);}

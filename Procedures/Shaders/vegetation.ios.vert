#version 300 es
precision highp float;
precision highp int;
// 16 floats/vertex; camera-relative position, world-space normal.
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
layout(location=2) in vec2 uv;
layout(location=3) in vec3 color;
layout(location=4) in vec4 properties;
layout(location=5) in float ridgeScale;
uniform mat4 viewProjection;
uniform mat4 sunViewProjection;
out vec3 worldPosition;
out vec3 surfaceNormal;
out vec2 leafUV;
out vec3 baseColor;
flat out vec4 material;
flat out float barkRidges;
out vec4 shadowPosition;
void main(){
 worldPosition=position;surfaceNormal=normal;leafUV=uv;baseColor=color;
 material=properties;barkRidges=ridgeScale;
 gl_Position=viewProjection*vec4(position,1.0);
 gl_Position.z=2.0*gl_Position.z-gl_Position.w;
 shadowPosition=sunViewProjection*vec4(position,1.0);
}

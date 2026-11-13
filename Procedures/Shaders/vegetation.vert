#version 450
// 16 floats/vertex; camera-relative position, world-space normal.
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
layout(location=2) in vec2 uv;
layout(location=3) in vec3 color;
layout(location=4) in vec4 properties;
layout(location=5) in float ridgeScale;
layout(set=0,binding=0,std140) uniform Scene {mat4 viewProjection;mat4 sunViewProjection;vec4 sunDirection_;vec4 sunColor_;vec4 ambientColor_;uvec4 options;} scene;
#define viewProjection scene.viewProjection
#define sunViewProjection scene.sunViewProjection
#define sunDirection scene.sunDirection_.xyz
#define sunColor scene.sunColor_.xyz
#define ambientColor scene.ambientColor_.xyz
#define shadowsEnabled (scene.options.x!=0u)

layout(location=0) out vec3 worldPosition;
layout(location=1) out vec3 surfaceNormal;
layout(location=2) out vec2 leafUV;
layout(location=3) out vec3 baseColor;
flat layout(location=4) out vec4 material;
flat layout(location=5) out float barkRidges;
layout(location=6) out vec4 shadowPosition;
void main(){
 worldPosition=position;surfaceNormal=normal;leafUV=uv;baseColor=color;
 material=properties;barkRidges=ridgeScale;
 gl_Position=viewProjection*vec4(position,1.0);
 gl_Position.y=-gl_Position.y;
 shadowPosition=sunViewProjection*vec4(position,1.0);
}

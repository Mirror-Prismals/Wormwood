#version 410 core
// Ported Cardinal water optics; continuous geometry, no cell slope IDs.
 in vec2 v_uv; uniform sampler2D scene_color; uniform sampler2D water_layer;
layout(location=0) out vec4 out_color;
void main(){vec2 uv=clamp(v_uv,vec2(0.001),vec2(0.999));vec3 scene=textureLod(scene_color,uv,0.0).rgb;
 vec4 layer=textureLod(water_layer,uv,0.0);out_color=vec4(mix(scene,layer.rgb,clamp(layer.a,0.0,1.0)),1.0);}

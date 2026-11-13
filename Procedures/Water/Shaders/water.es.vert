#version 300 es
precision highp float;
precision highp int;
// Ported Cardinal water optics; continuous geometry, no cell slope IDs.
layout(std140) uniform WaterUniforms {
 mat4 view; mat4 projection; vec4 camera_time; vec4 resolution_murkiness; vec4 light_sky;
 vec4 sky_top_neutral; vec4 sky_bottom_reflection; vec4 up_ripple_count;
 vec4 tangent_u; vec4 tangent_v; vec4 micro_phase; vec4 ripples[64];
} u;
layout(location=0) in vec4 in_position;
layout(location=1) in vec4 in_shading_normal;
layout(location=2) in vec4 in_geometric_normal;
layout(location=3) in vec4 in_axis_normal;
layout(location=4) in vec4 in_uv_flow;
layout(location=5) in vec4 in_depth_class_flags;
 out vec3 v_position;  out vec3 v_shading_normal;
 out vec3 v_geometric_normal;  out vec3 v_axis_normal;
 out vec4 v_uv_flow;  out vec2 v_depth;  flat out vec2 v_class_flags;
float ripple_height(vec2 p){
 float height=0.0;int count=min(64,int(max(0.0,u.up_ripple_count.w)));
 for(int i=0;i<count;++i){vec4 r=u.ripples[i];float age=max(0.0,r.z);float offset=length(p-r.xy)-age*2.65;
 height+=sin(offset*10.5)*exp(-offset*offset*3.4)*exp(-age*0.72)*max(0.0,r.w)*0.055;}return height;
}
float optical_waves(vec2 p,bool ripples){
 float t=u.camera_time.w*1.2;vec2 dir=vec2(1.0,0.7);
 float w=0.05*pow(sin(dot(p,dir*2.5)+u.micro_phase.x+t)*0.5+0.5,2.0);
 dir=mat2(cos(1.5),sin(1.5),-sin(1.5),cos(1.5))*dir;
 w+=0.025*pow(sin(dot(p,dir*2.5*1.8)+u.micro_phase.y+t*1.3)*0.5+0.5,1.5);
 dir=mat2(cos(2.2),sin(2.2),-sin(2.2),cos(2.2))*dir;
 w+=0.01*sin(dot(p,dir*2.5*3.5)+u.micro_phase.z+t*2.0);if(ripples)w+=ripple_height(p);return w;
}
float amplitude(int kind){if(kind==1)return 0.78;if(kind==3)return 1.12;if(kind==4)return 1.32;return 1.0;}
out float v_roof_cover;
void main(){
 v_roof_cover=in_shading_normal.w;
 vec3 up=normalize(u.up_ripple_count.xyz);vec3 geometric=normalize(in_geometric_normal.xyz);vec3 base=normalize(in_shading_normal.xyz);
 int kind=int(round(in_depth_class_flags.z));uint flags=uint(max(0.0,round(in_depth_class_flags.w)));
 bool ripples=kind<5;float scale=amplitude(kind);float height=optical_waves(in_uv_flow.xy,ripples);
 vec3 position=in_position.xyz;vec3 normal=base;if((flags&2u)!=0u)position+=up*height*scale*clamp(in_depth_class_flags.x/0.25,0.0,1.0);
 if(abs(dot(geometric,up))>0.05){float dx=(optical_waves(in_uv_flow.xy+vec2(0.035,0.0),ripples)-height)*scale/0.035;
 float dz=(optical_waves(in_uv_flow.xy+vec2(0.0,0.035),ripples)-height)*scale/0.035;
 normal=normalize(base/max(abs(dot(base,up)),0.0001)-u.tangent_u.xyz*dx-u.tangent_v.xyz*dz);}
 gl_Position=u.projection*u.view*vec4(position,1.0);v_position=position;v_shading_normal=normal;v_geometric_normal=geometric;
 v_axis_normal=normalize(in_axis_normal.xyz);v_uv_flow=in_uv_flow;v_depth=in_depth_class_flags.xy;v_class_flags=in_depth_class_flags.zw;
}

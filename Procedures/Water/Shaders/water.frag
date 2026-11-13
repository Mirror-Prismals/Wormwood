#version 450
// Ported Cardinal water optics; continuous geometry, no cell slope IDs.
layout(std140,set=0,binding=0) uniform WaterUniforms {
 mat4 view; mat4 projection; vec4 camera_time; vec4 resolution_murkiness; vec4 light_sky;
 vec4 sky_top_neutral; vec4 sky_bottom_reflection; vec4 up_ripple_count;
 vec4 tangent_u; vec4 tangent_v; vec4 micro_phase; vec4 ripples[64];
} u;
layout(location=0) in vec3 v_position; layout(location=1) in vec3 v_shading_normal;
layout(location=2) in vec3 v_geometric_normal; layout(location=3) in vec3 v_axis_normal;
layout(location=4) in vec4 v_uv_flow; layout(location=5) in vec2 v_depth; layout(location=6) flat in vec2 v_class_flags;
layout(set=0,binding=1) uniform sampler2D scene_color;layout(set=0,binding=2) uniform sampler2D reflection_color;layout(set=0,binding=3) uniform sampler2D scene_depth;
layout(location=0) out vec4 out_color;
vec3 facing(vec3 n,vec3 ray){return dot(n,-ray)<0.0?-n:n;}
layout(location=7) in float v_roof_cover;
void main(){
 vec3 ray=normalize(v_position-u.camera_time.xyz);vec3 normal=facing(normalize(v_shading_normal),ray);
 vec3 geometric=facing(normalize(v_geometric_normal),ray);vec3 axis=facing(normalize(v_axis_normal),ray);
 vec3 up=normalize(u.up_ripple_count.xyz);vec3 bend=refract(ray,geometric,1.0/1.33);
 float depth=clamp(abs(dot(geometric,up))>0.05?v_depth.x/max(abs(dot(bend,up)),0.12):v_depth.y/max(abs(dot(bend,axis)),0.12),0.05,24.0);
 vec3 absorption=exp(-depth*clamp(u.resolution_murkiness.z,0.0,1.0)*vec3(0.5,0.25,0.1));
 vec2 resolution=max(u.resolution_murkiness.xy,vec2(1.0));vec2 base_uv=clamp(gl_FragCoord.xy/resolution,vec2(0.001),vec2(0.999));
 if((gl_FragCoord.z-textureLod(scene_depth,base_uv,0.0).r)*u.micro_phase.w>0.0)discard;
 uint flags=uint(max(0.0,round(v_class_flags.y)));bool covered=(flags&1u)!=0u;vec3 local;
 if(covered)local=textureLod(scene_color,base_uv,0.0).rgb;
 else {vec3 red=normalize((u.view*vec4(refract(ray,normal,1.0/1.31),0.0)).xyz);
 vec3 blue=normalize((u.view*vec4(refract(ray,normal,1.0/1.35),0.0)).xyz);float pixels=clamp(depth*22.0,1.0,96.0);
 vec2 red_uv=clamp(base_uv+vec2(red.x,red.y*u.resolution_murkiness.w)*pixels/resolution,vec2(0.001),vec2(0.999));
 vec2 blue_uv=clamp(base_uv+vec2(blue.x,blue.y*u.resolution_murkiness.w)*pixels/resolution,vec2(0.001),vec2(0.999));
 vec3 a=textureLod(scene_color,red_uv,0.0).rgb;vec3 b=textureLod(scene_color,blue_uv,0.0).rgb;local=vec3(a.r,(a.g+b.g)*0.5,b.b);}
 // Scattering and specular require incident light; water is not emissive.
 float exposure=u.tangent_u.w>0.5?1.0:1.0-clamp(v_roof_cover,0.0,1.0);
 float sky_luminance=max(u.sky_top_neutral.r,max(u.sky_top_neutral.g,u.sky_top_neutral.b));
 float incident=u.tangent_u.w>0.5?1.0:clamp(sky_luminance,0.0,1.0)*exposure;
 vec3 refracted=local*absorption+(vec3(1.0)-absorption)*0.15*incident;vec3 reflection=reflect(ray,normal);
 vec3 reflected_view=normalize((u.view*vec4(reflection,0.0)).xyz);float elevation=max(0.0,dot(reflection,up));
 vec3 neutral=mix(vec3(0.88,0.93,0.98),vec3(0.98,0.99,1.0),elevation);
 vec3 live=mix(u.sky_bottom_reflection.rgb,u.sky_top_neutral.rgb,elevation);
 float sky_weight=clamp(u.light_sky.w,0.0,1.0);float neutral_weight=(1.0-sky_weight)*clamp(u.sky_top_neutral.w,0.0,1.0);
 float opacity=sky_weight+neutral_weight;vec3 sky=(neutral*neutral_weight+live*sky_weight)/max(opacity,0.0001);
 if(u.sky_bottom_reflection.w>0.5&&!covered){vec2 uv=clamp(base_uv+vec2(reflected_view.x,reflected_view.y*u.resolution_murkiness.w)*0.03,vec2(0.001),vec2(0.999));
 sky=mix(sky,textureLod(reflection_color,uv,0.0).rgb,0.6);opacity=mix(opacity,1.0,0.6);}
 // Preserve reflected colors without the later Cardinal blue tint.
 float fresnel=0.02+0.98*pow(1.0-max(dot(-ray,normal),0.0),5.0);float spec=pow(max(dot(reflect(ray,normal),normalize(u.light_sky.xyz)),0.0),128.0);
 vec3 color=mix(refracted,sky*exposure,clamp(fresnel*opacity,0.0,1.0))+vec3(spec*0.5*incident);
 if(int(round(v_class_flags.x))==3&&length(v_uv_flow.zw)>0.1){vec2 direction=normalize(v_uv_flow.zw);vec2 across=vec2(-direction.y,direction.x);
 float along=dot(v_uv_flow.xy,direction);float side=dot(v_uv_flow.xy,across);float t=u.camera_time.w;
 float broad=sin(along*0.82-t*2.15+sin(side*0.31)*0.75);float fine=sin(along*1.73-t*3.41+sin(side*0.57+t*0.19));
 color=clamp(color*(1.0+(broad*0.65+fine*0.35)*0.065),vec3(0.0),vec3(1.0));}
 // RGB already combines transmission and reflection; compose it once.
 if(u.tangent_v.w>0.5&&u.tangent_u.w<0.5){color*=1.0-smoothstep(24.0,34.0,length(v_position-u.camera_time.xyz));}
 out_color=vec4(color,1.0);
}

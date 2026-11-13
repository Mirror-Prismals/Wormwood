// Cardinal water optics port. Continuous normals/depth replace cell slope IDs.
struct Uniforms {
    view:mat4x4<f32>, projection:mat4x4<f32>,
    camera_time:vec4<f32>, resolution_murkiness:vec4<f32>, light_sky:vec4<f32>,
    sky_top_neutral:vec4<f32>, sky_bottom_reflection:vec4<f32>, up_ripple_count:vec4<f32>,
    tangent_u:vec4<f32>, tangent_v:vec4<f32>, micro_phase:vec4<f32>, ripples:array<vec4<f32>,64>,
};
@group(0) @binding(0) var<uniform> u:Uniforms;
@group(0) @binding(1) var scene_sampler:sampler;
@group(0) @binding(2) var scene_color:texture_2d<f32>;
@group(0) @binding(3) var reflection_color:texture_2d<f32>;
@group(0) @binding(4) var scene_depth:texture_depth_2d;
struct VertexIn {
    @location(0) position:vec4<f32>, @location(1) shading_normal:vec4<f32>,
    @location(2) geometric_normal:vec4<f32>, @location(3) axis_normal:vec4<f32>,
    @location(4) uv_flow:vec4<f32>, @location(5) depth_class_flags:vec4<f32>,
};
struct Varying {
    @builtin(position) clip:vec4<f32>, @location(0) position:vec3<f32>,
    @location(1) shading_normal:vec3<f32>, @location(2) geometric_normal:vec3<f32>,
    @location(3) axis_normal:vec3<f32>, @location(4) uv_flow:vec4<f32>,
    @location(5) depth:vec2<f32>, @location(6) @interpolate(flat) class_flags:vec2<f32>,
    @location(7) roof_cover:f32,
};
fn ripple_height(p:vec2<f32>)->f32 {
    var height=0.0;let count=min(64u,u32(max(0.0,u.up_ripple_count.w)));
    for(var i=0u;i<count;i=i+1u){
        let r=u.ripples[i];let offset=length(p-r.xy)-max(0.0,r.z)*2.65;
        height=height+sin(offset*10.5)*exp(-offset*offset*3.4)*exp(-max(0.0,r.z)*0.72)*max(0.0,r.w)*0.055;
    }
    return height;
}
fn optical_waves(p:vec2<f32>,ripples:bool)->f32 {
    let t=u.camera_time.w*1.2;var dir=vec2<f32>(1.0,0.7);
    var w=0.05*pow(sin(dot(p,dir*2.5)+u.micro_phase.x+t)*0.5+0.5,2.0);
    dir=mat2x2<f32>(vec2<f32>(cos(1.5),sin(1.5)),vec2<f32>(-sin(1.5),cos(1.5)))*dir;
    w=w+0.025*pow(sin(dot(p,dir*2.5*1.8)+u.micro_phase.y+t*1.3)*0.5+0.5,1.5);
    dir=mat2x2<f32>(vec2<f32>(cos(2.2),sin(2.2)),vec2<f32>(-sin(2.2),cos(2.2)))*dir;
    w=w+0.01*sin(dot(p,dir*2.5*3.5)+u.micro_phase.z+t*2.0);
    if(ripples){w=w+ripple_height(p);}return w;
}
fn amplitude(kind:i32)->f32 {
    if(kind==1){return 0.78;}if(kind==3){return 1.12;}if(kind==4){return 1.32;}return 1.0;
}
@vertex fn vs_main(input:VertexIn)->Varying {
    var out:Varying;let up=normalize(u.up_ripple_count.xyz);
    let geometric=normalize(input.geometric_normal.xyz);let base=normalize(input.shading_normal.xyz);
    let kind=i32(round(input.depth_class_flags.z));let flags=u32(max(0.0,round(input.depth_class_flags.w)));
    let ripples=kind<5;let scale=amplitude(kind);let height=optical_waves(input.uv_flow.xy,ripples);
    var position=input.position.xyz;var normal=base;
    // Geometry chooses explicitly which free surfaces move. Sloped boundaries,
    // interfaces and volume sides do not deform into cracks through prototype IDs.
    if((flags&2u)!=0u){position=position+up*height*scale*clamp(input.depth_class_flags.x/0.25,0.0,1.0);}
    if(abs(dot(geometric,up))>0.05){
        let dx=(optical_waves(input.uv_flow.xy+vec2<f32>(0.035,0.0),ripples)-height)*scale/0.035;
        let dz=(optical_waves(input.uv_flow.xy+vec2<f32>(0.0,0.035),ripples)-height)*scale/0.035;
        normal=normalize(base/max(abs(dot(base,up)),0.0001)-u.tangent_u.xyz*dx-u.tangent_v.xyz*dz);
    }
    out.clip=u.projection*u.view*vec4<f32>(position,1.0);out.position=position;
    out.shading_normal=normal;out.geometric_normal=geometric;out.axis_normal=normalize(input.axis_normal.xyz);
    out.roof_cover=input.shading_normal.w;out.uv_flow=input.uv_flow;out.depth=input.depth_class_flags.xy;out.class_flags=input.depth_class_flags.zw;
    return out;
}
fn facing(n:vec3<f32>,ray:vec3<f32>)->vec3<f32>{return select(n,-n,dot(n,-ray)<0.0);}
@fragment fn fs_main(input:Varying)->@location(0) vec4<f32> {
    let ray=normalize(input.position-u.camera_time.xyz);let normal=facing(normalize(input.shading_normal),ray);
    let geometric=facing(normalize(input.geometric_normal),ray);let axis=facing(normalize(input.axis_normal),ray);
    let up=normalize(u.up_ripple_count.xyz);let bend=refract(ray,geometric,1.0/1.33);
    let body=input.depth.y/max(abs(dot(bend,axis)),0.12);
    let top=input.depth.x/max(abs(dot(bend,up)),0.12);
    let depth=clamp(select(body,top,abs(dot(geometric,up))>0.05),0.05,24.0);
    let absorption=exp(-depth*clamp(u.resolution_murkiness.z,0.0,1.0)*vec3<f32>(0.5,0.25,0.1));
    let resolution=max(u.resolution_murkiness.xy,vec2<f32>(1.0));
    let base_uv=clamp(input.clip.xy/resolution,vec2<f32>(0.001),vec2<f32>(0.999));
    let opaque_depth=textureLoad(scene_depth,vec2<i32>(input.clip.xy),0);
    if((input.clip.z-opaque_depth)*u.micro_phase.w>0.0){discard;}
    let flags=u32(max(0.0,round(input.class_flags.y)));let covered=(flags&1u)!=0u;
    var local:vec3<f32>;
    if(covered){local=textureSampleLevel(scene_color,scene_sampler,base_uv,0.0).rgb;}
    else{
        let red=normalize((u.view*vec4<f32>(refract(ray,normal,1.0/1.31),0.0)).xyz);
        let blue=normalize((u.view*vec4<f32>(refract(ray,normal,1.0/1.35),0.0)).xyz);
        let pixels=clamp(depth*22.0,1.0,96.0);
        let red_uv=clamp(base_uv+vec2<f32>(red.x,red.y*u.resolution_murkiness.w)*pixels/resolution,vec2<f32>(0.001),vec2<f32>(0.999));
        let blue_uv=clamp(base_uv+vec2<f32>(blue.x,blue.y*u.resolution_murkiness.w)*pixels/resolution,vec2<f32>(0.001),vec2<f32>(0.999));
        let a=textureSampleLevel(scene_color,scene_sampler,red_uv,0.0).rgb;
        let b=textureSampleLevel(scene_color,scene_sampler,blue_uv,0.0).rgb;
        local=vec3<f32>(a.r,(a.g+b.g)*0.5,b.b);
    }
    // Scattering and specular require incident light; water is not emissive.
    let exposure=select(1.0-clamp(input.roof_cover,0.0,1.0),1.0,u.tangent_u.w>0.5);
    let sky_luminance=max(u.sky_top_neutral.r,max(u.sky_top_neutral.g,u.sky_top_neutral.b));
    let incident=select(clamp(sky_luminance,0.0,1.0)*exposure,1.0,u.tangent_u.w>0.5);
    let refracted=local*absorption+(vec3<f32>(1.0)-absorption)*0.15*incident;
    let reflection=reflect(ray,normal);let reflected_view=normalize((u.view*vec4<f32>(reflection,0.0)).xyz);
    let elevation=max(0.0,dot(reflection,up));
    let neutral=mix(vec3<f32>(0.88,0.93,0.98),vec3<f32>(0.98,0.99,1.0),elevation);
    let live=mix(u.sky_bottom_reflection.rgb,u.sky_top_neutral.rgb,elevation);
    let sky_weight=clamp(u.light_sky.w,0.0,1.0);let neutral_weight=(1.0-sky_weight)*clamp(u.sky_top_neutral.w,0.0,1.0);
    var opacity=sky_weight+neutral_weight;var sky=(neutral*neutral_weight+live*sky_weight)/max(opacity,0.0001);
    if(u.sky_bottom_reflection.w>0.5&&!covered){
        let uv=clamp(base_uv+vec2<f32>(reflected_view.x,reflected_view.y*u.resolution_murkiness.w)*0.03,vec2<f32>(0.001),vec2<f32>(0.999));
        sky=mix(sky,textureSampleLevel(reflection_color,scene_sampler,uv,0.0).rgb,0.6);opacity=mix(opacity,1.0,0.6);
    }
    // Preserve reflected colors without the later Cardinal blue tint.
    let fresnel=0.02+0.98*pow(1.0-max(dot(-ray,normal),0.0),5.0);
    let spec=pow(max(dot(reflect(ray,normal),normalize(u.light_sky.xyz)),0.0),128.0);
    var color=mix(refracted,sky*exposure,clamp(fresnel*opacity,0.0,1.0))+vec3<f32>(spec*0.5*incident);
    if(i32(round(input.class_flags.x))==3&&length(input.uv_flow.zw)>0.1){
        let direction=normalize(input.uv_flow.zw);let across=vec2<f32>(-direction.y,direction.x);
        let along=dot(input.uv_flow.xy,direction);let side=dot(input.uv_flow.xy,across);let t=u.camera_time.w;
        let broad=sin(along*0.82-t*2.15+sin(side*0.31)*0.75);
        let fine=sin(along*1.73-t*3.41+sin(side*0.57+t*0.19));
        color=clamp(color*(1.0+(broad*0.65+fine*0.35)*0.065),vec3<f32>(0.0),vec3<f32>(1.0));
    }
    // RGB already contains transmitted scene plus Fresnel reflection.
    // The composite must use this complete result once; low layer alpha would
    // suppress refraction, waves and reflected highlights a second time.
    if(u.tangent_v.w>0.5 && u.tangent_u.w<0.5){color=color*(1.0-smoothstep(24.0,34.0,length(input.position-u.camera_time.xyz)));}
    return vec4<f32>(color,1.0);
}

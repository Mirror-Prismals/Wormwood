// Complete opaque-scene/water-coverage composition. Other global camera grading stays upstream.
@group(0) @binding(0) var scene_sampler:sampler;
@group(0) @binding(1) var scene_color:texture_2d<f32>;
@group(0) @binding(2) var water_layer:texture_2d<f32>;
struct Output{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>};
@vertex fn vs_main(@builtin(vertex_index) id:u32)->Output {
    let positions=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    var out:Output;let p=positions[id];out.position=vec4<f32>(p,0.0,1.0);out.uv=vec2<f32>(p.x*0.5+0.5,0.5-p.y*0.5);return out;
}
@fragment fn fs_main(input:Output)->@location(0) vec4<f32>{
    let uv=clamp(input.uv,vec2<f32>(0.001),vec2<f32>(0.999));
    let scene=textureSampleLevel(scene_color,scene_sampler,uv,0.0).rgb;
    let layer=textureSampleLevel(water_layer,scene_sampler,uv,0.0);
    return vec4<f32>(mix(scene,layer.rgb,clamp(layer.a,0.0,1.0)),1.0);
}

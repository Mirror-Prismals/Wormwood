// Premultiplied scene-linear Cycles pixels, stored bottom-up. The artwork
// loads once. Only the destination rectangle changes when gain changes.
fn retainedFaderCap(point:vec3<f32>,background:vec4<f32>)->vec4<f32>{
 let header=gemRow(43152);
 if(header.w<0.5||header.x<1.0){return background;}
 let clip=gemRow(43164);
 if(any(point.xy<clip.xy)||any(point.xy>clip.zw)){return background;}
 let base=i32(gemRow(43158).y);
 for(var i:i32=0;i<i32(header.x);i++){
  let at=43160+6*i;
  let knob=gemRow(at+1);
  if(any(knob.zw<=vec2<f32>(0.0))){continue;}
  let uv=(point.xy-knob.xy)/knob.zw+0.5;
  if(any(uv<vec2<f32>(0.0))||any(uv>=vec2<f32>(1.0))){continue;}
  let p=clamp(uv*128.0-0.5,vec2<f32>(0.0),vec2<f32>(127.0));
  let lo=vec2<i32>(floor(p));let hi=min(lo+1,vec2<i32>(127));let f=fract(p);
  let a=mix(gemRow(base+128*lo.y+lo.x),gemRow(base+128*lo.y+hi.x),f.x);
  let b=mix(gemRow(base+128*hi.y+lo.x),gemRow(base+128*hi.y+hi.x),f.x);
  let color=mix(a,b,f.y);let pose=gemRow(at+3);
  let depth=point.z-(pose.x+(0.015-header.z)*pose.y);
  if(color.w>0.0&&depth<background.w){
   return vec4<f32>(color.xyz+(1.0-color.w)*background.xyz,depth);
  }
 }
 return background;
}

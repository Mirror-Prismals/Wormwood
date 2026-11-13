// Interpolate radiance only within the same visible surface when a panel
// transform lands between captured pixels. Keep the original depth winner.
fn panelAtlasSample(group:i32,source:vec2<f32>,rect:vec4<f32>,xy:vec2<i32>,sample:i32)->vec4<f32>{
 let offset=vec2<i32>(retainedMeta.rows[5u+u32(group)*4u].xy);
 let center=textureLoad(retainedIn,xy+offset,sample,0);
 let uv=source-rect.xy;
 if(group<1||group>4||center.w>=1e19||all(abs(uv-vec2<f32>(xy))<vec2<f32>(0.001))){return center;}
 let lo=vec2<i32>(floor(uv));let f=fract(uv);
 let tolerance=4.0*gemRow(3).w/gemRow(8).y;
 var color=vec3<f32>(0.0);var sum=0.0;
 for(var j:i32=0;j<2;j++){for(var i:i32=0;i<2;i++){
  let texel=lo+vec2<i32>(i,j);
  if(any(texel<vec2<i32>(0))||any(texel>=vec2<i32>(rect.zw))){continue;}
  let v=textureLoad(retainedIn,texel+offset,sample,0);
  if(abs(v.w-center.w)<=tolerance){
   let w=select(1.0-f.x,f.x,i==1)*select(1.0-f.y,f.y,j==1);
   color+=v.xyz*w;sum+=w;
  }
 }}
 if(sum<=0.0){return center;}
 return vec4<f32>(color/sum,center.w);
}


// Keep the crown and clipped corners at their physical thickness while the
// face grows. Stretch only the interior of the retained panel image.
fn retainedPanelSourceWithBands(p:vec2<f32>,old:vec4<f32>,now:vec4<f32>,bands:vec2<f32>)->vec2<f32>{
 let affine=(p+vec2<f32>(0.5)-now.xy)*old.zw/max(now.zw,vec2<f32>(0.000001))+old.xy-vec2<f32>(0.5);
 if(all(old.zw==now.zw)){return affine;}
 if(any(bands<=vec2<f32>(0.0))){return affine;}
 let oldBand=min(vec2<f32>(bands.x),old.zw*0.49);
 let newBand=min(vec2<f32>(bands.y),now.zw*0.49);
 let local=p+vec2<f32>(0.5)-now.xy;let distance=abs(local);
 let oldInner=old.zw*0.5-oldBand;let newInner=now.zw*0.5-newBand;
 let interior=distance*oldInner/max(newInner,vec2<f32>(0.000001));
 let edge=oldInner+(distance-newInner)*oldBand/newBand;
 return sign(local)*select(interior,edge,distance>newInner)+old.xy-vec2<f32>(0.5);
}

fn retainedPanelSource(group:i32,p:vec2<f32>,old:vec4<f32>,now:vec4<f32>)->vec2<f32>{
 if(group<1||group>4){return (p+vec2<f32>(0.5)-now.xy)*old.zw/max(now.zw,vec2<f32>(0.000001))+old.xy-vec2<f32>(0.5);}
 return retainedPanelSourceWithBands(p,old,now,retainedMeta.rows[5u+u32(group)*4u].zw);
}

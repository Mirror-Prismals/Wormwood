// FURNITURE_IMAGE_COMPOSITION_V1
@group(0) @binding(18) var furnitureBodyOut:texture_storage_2d_array<rgba32float,write>;
@group(0) @binding(19) var furnitureBodyIn:texture_2d_array<f32>;
@group(0) @binding(20) var furnitureOwnerOut:texture_storage_2d_array<r32uint,write>;
@group(0) @binding(21) var furnitureOwnerIn:texture_2d_array<u32>;
var<private> furnitureBodyOnly:bool=false;
fn furnitureEnabled()->bool{return retainedMeta.rows[0u].x==1.0;}
fn furnitureCapture(pixel:vec2<u32>,group:i32,sample:i32,origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>,color:vec4<f32>){
 if(!furnitureEnabled()||(group!=2&&group!=4)){return;}
 let offset=vec2<i32>(retainedMeta.rows[5+group*4].xy);let xy=vec2<i32>(pixel)+offset;
 let owner=gemNearest(origin,direction,-1,1e20);
 textureStore(furnitureOwnerOut,xy,sample,vec4<u32>(u32(owner.panel+1),0u,0u,0u));
 var body=color;
 if(owner.panel!=group-1){
  furnitureBodyOnly=true;
  let hit=gemNearest(origin,direction,-1,1e20);body=vec4<f32>(0.0,0.0,0.0,1e20);
  if(hit.panel>=0){body=vec4<f32>(gemTrace(origin,direction,random),hit.distance);}
  furnitureBodyOnly=false;
 }
 textureStore(furnitureBodyOut,xy,sample,body);
}
fn furnitureMatches(owner:i32,kind:i32,index:i32)->bool{
 if(kind==1){return owner>=4&&owner<32772&&(owner-4)/256==index;}
 if(kind==2){return owner>=800000&&owner<853760&&(owner-800000)/256==index;}
 return owner>=700000&&owner<704096&&(owner-700000)/16==index;
}
fn furnitureComposite(p:vec2<f32>,sample:i32,initial:vec4<f32>)->vec4<f32>{
 if(!furnitureEnabled()){return initial;}var best=initial;
 let bottom=retainedMeta.rows[2368u];let right=retainedMeta.rows[2369u];
 if(!(all(p>=bottom.xy)&&all(p<bottom.zw))&&!(all(p>=right.xy)&&all(p<right.zw))){return best;}
 let rayOrigin=retainedRay(p,sample);
 for(var i:u32=0u;i<384u;i++){
  if(f32(i)>=retainedMeta.rows[0u].y){break;}
  let at=64u+6u*i;let now=retainedMeta.rows[at];let scale=retainedMeta.rows[at+1u];let old=retainedMeta.rows[at+2u];let oldScale=retainedMeta.rows[at+3u];let bounds=retainedMeta.rows[at+4u];
  if(any(p<bounds.xy)||any(p>=bounds.xy+bounds.zw)||oldScale.w!=1.0){continue;}
  let clip=retainedMeta.rows[at+5u];if(any(rayOrigin.xy<clip.xy)||any(rayOrigin.xy>clip.zw)){continue;}
  var source=p;
  if(any(now.xyz!=old.xyz)||any(scale.xyz!=oldScale.xyz)){
   let local=(rayOrigin.xy-now.xy)/scale.xy;let point=old.xy+local*oldScale.xy;
   let xi=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sample));
   source=vec2<f32>((point.x/gemRow(2).w+1.0)*gemRow(8).x*0.5,(1.0-point.y/gemRow(3).w)*gemRow(8).y*0.5)-vec2<f32>(xi.x,1.0-xi.y);
  }
  let group=i32(old.w);let rect=retainedMeta.rows[2+group*4];let xy=vec2<i32>(floor(source-rect.xy+0.5));
  if(any(xy<vec2<i32>(0))||any(xy>=vec2<i32>(rect.zw))){continue;}
  let texel=xy+vec2<i32>(retainedMeta.rows[5+group*4].xy);
  let owner=i32(textureLoad(furnitureOwnerIn,texel,sample,0).x)-1;
  if(!furnitureMatches(owner,i32(now.w),i32(scale.w))){continue;}
  var value=textureLoad(retainedIn,texel,sample,0);
  if(value.w>=1e19){continue;}
  if(now.z!=old.z||scale.z!=oldScale.z){let localZ=(gemRow(1).z-value.w-old.z)/oldScale.z;value.w=gemRow(1).z-(now.z+localZ*scale.z);}
  if(value.w<best.w){best=value;}
 }
 return best;
}

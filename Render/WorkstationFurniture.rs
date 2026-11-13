//! Independently placed original control pixels. The old panel image remains
//! the source; an owner atlas separates each authored component from its panel.
pub(super) const MAX:usize=384;
pub(super) const BASE:usize=64;
pub(super) const STRIDE:usize=6;
pub(super) const META_ROWS:usize=BASE+MAX*STRIDE+2;
#[derive(Clone)]
struct Item{kind:u32,index:u32,group:usize,pose:[f32;3],scale:[f32;3],lo:[f32;3],hi:[f32;3],clip:[f32;4]}
fn pose_row(r:&[[f32;4]],i:usize)->usize{
 if i>=125{r[44592][0]as usize+5*(i-125)}else if i>=65{r[44288][0]as usize+5*(i-65)}else if i>=61{r[232][0]as usize+32+2*(i-61)}else if i>=29{r[247][0]as usize+2*(i-29)}else if i>=13{r[232][0]as usize+2*(i-13)}else if i>=10{226+2*(i-10)}else{192+2*i}
}
fn items(r:&[[f32;4]])->Option<Vec<Item>>{
 if r.len()<=45754{return None;}let mut out=Vec::new();
 let start=r[12][2]as usize;let end=r[12][3]as usize;
 if start<256||end>r.len()||end<start||(end-start)%2!=0{return None;}
 for at in (start..end).step_by(2){let lo=r[at];let hi=r[at+1];if hi[3]<0.{continue;}let i=hi[3]as usize;
  if i<4||(10..13).contains(&i)||(29..61).contains(&i){continue;}
  if i>=128{return None;}let p=*r.get(pose_row(r,i))?;if p[3]<=0.{continue;}
  out.push(Item{kind:1,index:i as u32,group:if i>=65{4}else{2},pose:[p[0],p[1],p[2]],scale:[p[3];3],lo:[lo[0],lo[1],lo[2]],hi:[hi[0],hi[1],hi[2]],clip:[-1e10,-1e10,1e10,1e10]});
 }
 let count=r[43152][0]as usize;if count>70{return None;}
 for i in 0..count{let at=43160+6*i;for part in [0,2]{
  if r[at+5][0]>1.5&&part!=2{continue;}let rect=r[at+part];let nominal=if part==0{[0.04,0.18]}else{[0.01,0.156]};let s=[rect[2]/nominal[0],rect[3]/nominal[1],r[at+3][1]];
  if s.iter().any(|v|*v<=1e-9){continue;}
  let low=if part==2{r[43154][3]}else{r[43152][2]};let p=[rect[0],rect[1],r[at+3][0]-low*s[2]];
  let root=if part==0{r[43153][0]}else{r[43154][0]}as usize;let a=*r.get(root)?;let b=*r.get(root+1)?;
  let lo=std::array::from_fn(|j|p[j]+a[j]*s[j]-0.00001);let hi=std::array::from_fn(|j|p[j]+b[j]*s[j]+0.00001);
  out.push(Item{kind:2,index:(i*3+part)as u32,group:2,pose:p,scale:s,lo,hi,clip:r[at+4]});
 }}
 let count=r[255][2]as usize;let base=r[255][0]as usize;if count>12{return None;}
 for i in 0..count{let at=base+12+3*i;let p=*r.get(at)?;let size=*r.get(at+2)?;if p[3]<=0.{continue;}
  let analytic=r[at+1][3]< -0.5;let factor=if analytic{1.}else{0.5};let half=[size[0]*factor*p[3],size[1]*factor*p[3],size[2]*p[3]];let lo=std::array::from_fn(|j|p[j]-half[j]-0.00001);let hi=std::array::from_fn(|j|p[j]+half[j]+0.00001);
  out.push(Item{kind:3,index:(192+i)as u32,group:2,pose:[p[0],p[1],p[2]],scale:[p[3];3],lo,hi,clip:*r.get(base)?});
 }
 (out.len()<=MAX&&out.iter().all(|i|i.lo.iter().zip(i.hi).all(|(a,b)|a.is_finite()&&b.is_finite()&&*a<=b))).then_some(out)
}
/// Preserve the exact pose associated with each captured group. Logical IDs,
/// rather than changing list offsets, carry identities across visibility edits.
pub(super) fn update(rows:&mut [[f32;4];META_ROWS],previous:&[[f32;4];META_ROWS],frame:&[[f32;4]],capture:Option<u32>)->bool{
 let Some(items)=items(frame)else{rows[0]=[0.;4];return false;};
 let old_count=(previous[0][1]as usize).min(MAX);let mut old=std::collections::BTreeMap::new();
 if previous[0][0]==1.{for i in 0..old_count{let at=BASE+i*STRIDE;old.insert((previous[at][3]as u32,previous[at+1][3]as u32),at);}}
 let project=|p:[f32;3]|[(p[0]/frame[2][3]+1.)*frame[8][0]*0.5,(1.-p[1]/frame[3][3])*frame[8][1]*0.5];
 rows[0]=[1.,items.len()as f32,0.,0.];
 let mut unions:[[f32;4];2]=[[f32::INFINITY,f32::INFINITY,f32::NEG_INFINITY,f32::NEG_INFINITY];2];
 for (i,item)in items.iter().enumerate(){let at=BASE+i*STRIDE;let a=project(item.lo);let b=project(item.hi);
  rows[at]=[item.pose[0],item.pose[1],item.pose[2],item.kind as f32];rows[at+1]=[item.scale[0],item.scale[1],item.scale[2],item.index as f32];
  rows[at+4]=[a[0].floor()-3.,b[1].floor()-3.,(b[0]-a[0]).ceil()+6.,(a[1]-b[1]).ceil()+6.];rows[at+5]=item.clip;
  let u=&mut unions[usize::from(item.group==4)];let b=rows[at+4];u[0]=u[0].min(b[0]);u[1]=u[1].min(b[1]);u[2]=u[2].max(b[0]+b[2]);u[3]=u[3].max(b[1]+b[3]);
  let refreshed=capture.is_some_and(|m|m==0||m&(1<<item.group)!=0);
  if !refreshed {if let Some(&source)=old.get(&(item.kind,item.index)){rows[at+2]=previous[source+2];rows[at+3]=previous[source+3];continue;}}
  rows[at+2]=[item.pose[0],item.pose[1],item.pose[2],item.group as f32];rows[at+3]=[item.scale[0],item.scale[1],item.scale[2],1.];
 }
 for i in 0..2{let u=unions[i];rows[BASE+MAX*STRIDE+i]=if u[0].is_finite(){u}else{[0.;4]};}
 true
}

use super::*;
pub(super) struct Images{api:&'static Api,pub textures:[WGPUTexture;2],pub views:[WGPUTextureView;2]}
impl Images{
 pub unsafe fn new(api:&'static Api,device:WGPUDevice,extent:[u32;2])->Result<Self,String>{
  let mut out=Self{api,textures:[ptr::null_mut();2],views:[ptr::null_mut();2]};
  for (i,format)in[WGPUTextureFormat_RGBA32Float,WGPUTextureFormat_R32Uint].into_iter().enumerate(){
   out.textures[i]=(api.wgpuDeviceCreateTexture)(device,&WGPUTextureDescriptor{label:view("Independent control body/owner images"),usage:WGPUTextureUsage_StorageBinding|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst,dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:extent[0],height:extent[1],depthOrArrayLayers:2},format,mipLevelCount:1,sampleCount:1,..Default::default()});
   if out.textures[i].is_null(){return Err("control image allocation failed".into());}
   out.views[i]=(api.wgpuTextureCreateView)(out.textures[i],&WGPUTextureViewDescriptor{dimension:WGPUTextureViewDimension_2DArray,arrayLayerCount:2,mipLevelCount:1,aspect:WGPUTextureAspect_All,..Default::default()});
   if out.views[i].is_null(){return Err("control image view failed".into());}
  }check_errors()?;Ok(out)
 }
 pub fn bindings(&self,layouts:&mut Vec<WGPUBindGroupLayoutEntry>,entries:&mut Vec<WGPUBindGroupEntry>){
  for i in 0..2{
   layouts.push(WGPUBindGroupLayoutEntry{binding:18+2*i,visibility:WGPUShaderStage_Compute,storageTexture:WGPUStorageTextureBindingLayout{access:WGPUStorageTextureAccess_WriteOnly,format:if i==0{WGPUTextureFormat_RGBA32Float}else{WGPUTextureFormat_R32Uint},viewDimension:WGPUTextureViewDimension_2DArray,..Default::default()},..Default::default()});
   layouts.push(WGPUBindGroupLayoutEntry{binding:19+2*i,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:if i==0{WGPUTextureSampleType_UnfilterableFloat}else{WGPUTextureSampleType_Uint},viewDimension:WGPUTextureViewDimension_2DArray,multisampled:0,..Default::default()},..Default::default()});
   for binding in [18+2*i,19+2*i]{entries.push(WGPUBindGroupEntry{binding,textureView:self.views[i as usize],..Default::default()});}
  }
 }
}
impl Drop for Images{fn drop(&mut self){unsafe{for i in 0..2{if !self.views[i].is_null(){(self.api.wgpuTextureViewRelease)(self.views[i]);}if !self.textures[i].is_null(){(self.api.wgpuTextureRelease)(self.textures[i]);}}}}}
pub(super) fn source(input:&str)->Result<String,String>{
 let mut s=input.to_string();
 fn once(s:&mut String,a:&str,b:&str)->Result<(),String>{if s.matches(a).count()!=1{return Err(format!("furniture source changed: {a}"));}*s=s.replacen(a,b,1);Ok(())}
 let leaf="        if(retainedPrimary&&retainedControlGroup(instance)!=retainedGroup){continue;}";
 once(&mut s,leaf,&format!("{leaf}\n        if(retainedPrimary&&furnitureBodyOnly){{continue;}}"))?;
 let bottom="var bottomCount:i32=i32(gemRow(255).z);";
 once(&mut s,bottom,&format!("{bottom}\n    if(retainedPrimary&&furnitureBodyOnly){{bottomCount=0;}}"))?;
 once(&mut s,"if(!retainedPrimary||retainedGroup==2){mixer=", "if((!retainedPrimary||retainedGroup==2)&&!(retainedPrimary&&furnitureBodyOnly)){mixer=")?;
 let store=" textureStore(retainedOut,vec2<i32>(id.xy)+vec2<i32>(retainedMeta.rows[5u+u32(group)*4u].xy),sample,value);";
 once(&mut s,store,&format!(" furnitureCapture(id.xy,group,sample,origin,direction,random,value);\n{store}"))?;
 let playback=" let center=textureLoad(retainedIn,xy+offset,sample,0);";
 once(&mut s,playback,&format!(" var center=textureLoad(retainedIn,xy+offset,sample,0);\n if(furnitureEnabled()&&(group==2||group==4)){{center=textureLoad(furnitureBodyIn,xy+offset,sample,0);}}"))?;
 let v="  let v=textureLoad(retainedIn,texel+offset,sample,0);";
 once(&mut s,v,"  var v=textureLoad(retainedIn,texel+offset,sample,0);\n  if(furnitureEnabled()&&(group==2||group==4)){v=textureLoad(furnitureBodyIn,texel+offset,sample,0);}")?;
 once(&mut s,"  // Live scope/waveform/selection", "  best=furnitureComposite(p,sample,best);\n  // Live scope/waveform/selection")?;
 s.push_str(include_str!("WorkstationFurniture.wgsl"));
 let module=naga::front::wgsl::parse_str(&s).map_err(|e|e.emit_to_string(&s))?;
 naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).map_err(|e|format!("control image shader: {e:?}"))?;
 Ok(s)
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn component_motion_keeps_its_own_scale_and_capture_identity(){
  let mut frame=vec![[0.;4];50000];frame[2][3]=1.;frame[3][3]=1.;frame[8]=[960.,600.,16.,64.];frame[12][2]=1000.;frame[12][3]=1002.;
  frame[1000]=[-0.2,-0.1,0.01,1002.];frame[1001]=[0.2,0.1,0.05,4.];frame[200]=[0.,0.,0.03,1.];
  let zero=[[0.;4];META_ROWS];let mut captured=zero;assert!(update(&mut captured,&zero,&frame,Some(0)));assert_eq!(captured[0][1],1.);
  frame[200]=[0.25,0.3,0.025,0.5];let mut moving=captured;assert!(update(&mut moving,&captured,&frame,Some(1<<3)));
  assert_eq!(moving[BASE][..3],[0.25,0.3,0.025]);assert_eq!(moving[BASE+1][..3],[0.5;3]);
  assert_eq!(moving[BASE+2],captured[BASE+2]);assert_eq!(moving[BASE+3],captured[BASE+3]);
  let mut settled=moving;assert!(update(&mut settled,&moving,&frame,Some(0)));assert_eq!(settled[BASE+2][..3],[0.25,0.3,0.025]);assert_eq!(settled[BASE+3][..3],[0.5;3]);
 }
 #[test]fn detailed_component_capture_keeps_group_mask_and_validates(){
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  let base=super::super::source::source(original,true,false,true).unwrap();
  let detailed=super::super::source::panel_detail(&source(&base).unwrap(),original).unwrap();
  let tiled=super::super::incremental::tiled_source(&detailed).unwrap();
  assert!(tiled.contains("if(captureMask!=0u&&(captureMask&(1u<<u32(group)))==0u){return;}"));
  let module=naga::front::wgsl::parse_str(&tiled).unwrap();
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
 }
 #[test]fn production_furniture_shader_preserves_optics_and_validates(){
  let base=super::super::source::source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,true).unwrap();
  let shader=source(&base).unwrap();assert!(shader.contains("furnitureComposite(p,sample,best)"));
  assert!(shader.contains("textureStore(furnitureOwnerOut"));assert!(shader.contains("if(retainedPrimary&&furnitureBodyOnly){continue;}"));
  assert!(shader.contains("best=retainedFaderCap(origin,best)"));
 }
}

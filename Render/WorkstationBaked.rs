//! AUTHOR POLICY, 2026-11-08: the DAW is an image compositor.
//! Gem lighting is immutable artwork built OFFLINE by bake_daw_art.py. Never
//! invalidate it for movement, pressure, text, meters, clips, zoom, lighting or
//! settling. Slightly stale reflections/refraction are explicitly accepted.
//! Do not restore a live optical fallback to improve screenshot equivalence.
//! The 3D gem ruby remains the main/default theme. Cardinal 2D themes are opt-in.
//! The old optical renderer is retained only as an explicit research alternative.
use super::*;
use std::collections::BTreeMap;
#[path="BubbleArt.rs"]
mod bubble_art;
#[path="CardinalThemeAtlas.rs"]
mod cardinal_atlas;
struct Animation{row:usize,family:usize,age:f64,updated:f64,born:Option<f64>,phase:f64}
const STRIDE:usize=8;
const MAX:usize=8192;
const THEME_BASE:usize=MAX*STRIDE+4;
const METADATA_ROWS:usize=THEME_BASE+MAX*8+32;
pub(super) fn enabled()->bool{cfg!(target_os="macos")&&std::env::var("WORMWOOD_WORKSTATION_REFERENCE_OPTICS").as_deref()!=Ok("1")}
#[derive(Clone)]
struct Art{offset:usize,edge:usize,lo:[f32;3],hi:[f32;3]}
fn number(v:&Value,k:&str)->Result<usize,String>{v[k].as_u64().and_then(|n|usize::try_from(n).ok()).ok_or_else(||format!("invalid baked art {k}"))}
fn replace_body(s:&mut String,name:&str,body:&str)->Result<(),String>{
 let start=s.find(&format!("fn {name}(" )).ok_or_else(||format!("baked shader missing {name}"))?;
 let a=start+s[start..].find('{').ok_or("baked shader function body")?;let mut depth=1;let mut end=a+1;
 for (i,c) in s[a+1..].char_indices(){if c=='{'{depth+=1;}if c=='}'{depth-=1;}if depth==0{end=a+1+i;break;}}
 if depth!=0{return Err("baked shader unbalanced function".into());}s.replace_range(a+1..end,body);Ok(())
}
fn source(original:&str)->Result<String,String>{
 let mut s=original.to_owned();
 // Hard guard: even an accidental use of the old entry cannot trace optics
 // inside this module. Canonical optical source remains available to the
 // separately invoked offline baker, never the interactive compositor.
 replace_body(&mut s,"gemTrace","return vec3<f32>(0.0);")?;
 for name in ["gemEngravingLight","gemControlEngravingLight","gemBarEngravingLight","gemMixerEngravingLight","gemTimelineEngravingLight"]{
  replace_body(&mut s,name,"return gemRow(214).xyz*2.0;")?;
 }
 s.push_str(include_str!("WorkstationBaked.wgsl"));s.push_str(include_str!("CardinalTheme.wgsl"));Ok(s)
}
pub(super) struct Pipeline{
 api:&'static Api,device:WGPUDevice,shader:WGPUShaderModule,layout:WGPUBindGroupLayout,pipeline_layout:WGPUPipelineLayout,
 render:WGPURenderPipeline,overlay:WGPURenderPipeline,backdrop:WGPURenderPipeline,plain_screen:bool,group:WGPUBindGroup,images:WGPUBuffer,metadata:WGPUBuffer,
 bubble_bank:bubble_art::Bank,bubble_images:WGPUBuffer,animations:Vec<Animation>,births:BTreeMap<u64,Option<f64>>,started:std::time::Instant,last_animation:std::time::Instant,animation_uploads:u64,fixed_time:Option<f64>,
 cardinal_atlas:Option<cardinal_atlas::Atlas>,
 theme:Option<std::sync::Arc<crate::workstation::cardinal_theme::Theme>>,
 art:BTreeMap<(i64,usize),Art>,rows:Vec<[f32;4]>,count:u32,uploads:u64,draws:std::cell::Cell<u64>,missing:std::collections::BTreeSet<(i64,usize)>,bytes:usize,image_buffer_bytes:usize,auxiliary_buffer_bytes:usize,
}
fn r(frame:&[[f32;4]],at:usize)->[f32;4]{frame.get(at).copied().unwrap_or([0.;4])}
fn control_row(f:&[[f32;4]],i:usize)->usize{if i>=125{r(f,44592)[0]as usize+5*(i-125)}else if i>=65{r(f,44288)[0]as usize+5*(i-65)}else if i>=61{r(f,232)[0]as usize+32+2*(i-61)}else if i>=29{r(f,247)[0]as usize+2*(i-29)}else if i>=13{r(f,232)[0]as usize+2*(i-13)}else if i>=10{226+2*(i-10)}else{192+2*i}}
impl Pipeline{
 pub(super) unsafe fn new(c:&Client,root:&Path,original:&str)->Result<Self,String>{
  let theme=crate::workstation::cardinal_theme::selected(root)?;
  let path=root.join("Art/AudioWorkstation/Baked");let manifest:Value=serde_json::from_slice(&std::fs::read(path.join("manifest.json")).map_err(|e|format!("Baked DAW artwork missing; run offline bake: {e}"))?).map_err(|e|e.to_string())?;
  if manifest["schema"]!=1{return Err("unsupported baked DAW artwork schema".into());}
  let (cardinal_atlas,mut pixels)=if let Some(t)=&theme{let (a,p)=cardinal_atlas::Atlas::load(root,t)?;(Some(a),p)}else{(None,std::fs::read(path.join("images.rgba32f")).map_err(|e|e.to_string())?)};
  if pixels.is_empty()||pixels.len()%4!=0||pixels.len()>128*1024*1024{return Err("invalid baked DAW image storage".into());}
  let mut art=BTreeMap::new();
  for entry in manifest["entries"].as_array().ok_or("baked DAW entries missing")?{
   let key=entry["root"].as_i64().ok_or("baked art root")?;let material=number(entry,"material")?;let offset=number(entry,"offset")?;let edge=number(entry,"edge")?;
   let lo:[f32;3]=serde_json::from_value(entry["min"].clone()).map_err(|e|e.to_string())?;let hi:[f32;3]=serde_json::from_value(entry["max"].clone()).map_err(|e|e.to_string())?;
   if edge==0||edge>2048||(theme.is_none()&&offset.checked_add(edge*edge*2).is_none_or(|n|n>pixels.len()/16))||lo.iter().zip(hi).any(|(a,b)|!a.is_finite()||!b.is_finite()||*a>=b){return Err("invalid baked DAW image bounds".into());}
   art.insert((key,material),Art{offset,edge,lo,hi});
  }
  let (bubble_bank,mut bubble_pixels)=bubble_art::Bank::load_mode(root,theme.is_none())?;
  if theme.is_some(){bubble_pixels=std::fs::read(root.join("Art/AudioWorkstation/CardinalThemes/grain.rgba8")).map_err(|e|format!("Cardinal grain artwork missing: {e}"))?;if bubble_pixels.len()!=1024*1024*4{return Err("invalid Cardinal grain image size".into());}}
  let fixed_time=std::env::var("WORMWOOD_BUBBLE_ART_TIME").ok().and_then(|v|v.parse::<f64>().ok()).filter(|v|v.is_finite()&&(0. ..=1e9).contains(v));
  let disk_bytes=pixels.len();
  let image_pixels=if theme.is_some(){vec![0u8;16]}else{std::mem::take(&mut pixels)};
  if theme.is_some(){bubble_pixels.extend_from_slice(&pixels);}
  let a=c.api;let mut p=Self{cardinal_atlas,theme,bubble_bank,bubble_images:ptr::null_mut(),animations:Vec::new(),births:BTreeMap::new(),started:std::time::Instant::now(),last_animation:std::time::Instant::now(),animation_uploads:0,fixed_time,api:a,device:c.device,shader:ptr::null_mut(),layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),render:ptr::null_mut(),overlay:ptr::null_mut(),backdrop:ptr::null_mut(),plain_screen:false,group:ptr::null_mut(),images:ptr::null_mut(),metadata:ptr::null_mut(),art,rows:vec![],count:0,uploads:0,draws:std::cell::Cell::new(0),missing:Default::default(),bytes:disk_bytes,image_buffer_bytes:image_pixels.len(),auxiliary_buffer_bytes:bubble_pixels.len()};
  p.images=(a.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Immutable offline DAW artwork"),size:image_pixels.len()as u64,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
  p.metadata=(a.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Live sprite transforms, never lighting"),size:(METADATA_ROWS*16)as u64,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
  p.bubble_images=(a.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Offline Cycles translucent bubble frames"),size:bubble_pixels.len()as u64,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
  if p.images.is_null()||p.metadata.is_null()||p.bubble_images.is_null(){return Err("baked DAW buffer allocation failed".into());}
  (a.wgpuQueueWriteBuffer)(c.queue,p.images,0,image_pixels.as_ptr().cast(),image_pixels.len());
  (a.wgpuQueueWriteBuffer)(c.queue,p.bubble_images,0,bubble_pixels.as_ptr().cast(),bubble_pixels.len());
  let buffers=[c.vertex_buffer,c.texture_buffer,p.images,p.metadata,p.bubble_images];let sizes=[crate::scene_limits::SCENE_BUFFER_BYTES,c.texture_capacity as u64,image_pixels.len()as u64,(METADATA_ROWS*16)as u64,bubble_pixels.len()as u64];
  let entries:Vec<_>=(0..5).map(|i|WGPUBindGroupEntry{binding:i as u32,buffer:buffers[i],size:sizes[i],..Default::default()}).collect();
  let layouts:Vec<_>=(0..5).map(|i|WGPUBindGroupLayoutEntry{binding:i as u32,visibility:WGPUShaderStage_Vertex|WGPUShaderStage_Fragment,buffer:WGPUBufferBindingLayout{type_:WGPUBufferBindingType_ReadOnlyStorage,minBindingSize:sizes[i],..Default::default()},..Default::default()}).collect();
  p.layout=(a.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{entryCount:layouts.len(),entries:layouts.as_ptr(),..Default::default()});
  p.pipeline_layout=(a.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&p.layout,..Default::default()});
  p.group=(a.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout:p.layout,entryCount:entries.len(),entries:entries.as_ptr(),..Default::default()});
  let mut s=source(original)?;
  if p.theme.is_some(){s=s.replace("const CARDINAL_2D:bool=false;","const CARDINAL_2D:bool=true;");}let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(&s)};
  p.shader=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("DAW sprite compositor: NO live gem lighting")});
  let blend=WGPUBlendState{color:WGPUBlendComponent{operation:WGPUBlendOperation_Add,srcFactor:WGPUBlendFactor_One,dstFactor:WGPUBlendFactor_OneMinusSrcAlpha},alpha:WGPUBlendComponent{operation:WGPUBlendOperation_Add,srcFactor:WGPUBlendFactor_One,dstFactor:WGPUBlendFactor_OneMinusSrcAlpha}};
  let target=WGPUColorTargetState{format:c.format,blend:&blend,writeMask:WGPUColorWriteMask_All,..Default::default()};
  let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_True,depthCompare:WGPUCompareFunction_LessEqual,..Default::default()};
  for mode in 0..3{
   let overlay=mode!=0;
   let fragment=WGPUFragmentState{module:p.shader,entryPoint:view(if mode==2{"fs_baked_backdrop"}else if overlay{"fs_baked_overlay"}else{"fs_baked"}),targetCount:1,targets:&target,..Default::default()};
   let pipeline=(a.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{label:view("Baked DAW image composition"),layout:p.pipeline_layout,vertex:WGPUVertexState{module:p.shader,entryPoint:view(if overlay{"vs_baked_overlay"}else{"vs_baked"}),..Default::default()},fragment:&fragment,depthStencil:&depth,primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},..Default::default()});
   if pipeline.is_null(){return Err("baked DAW pipeline creation failed".into());}if mode==2{p.backdrop=pipeline;}else if overlay{p.overlay=pipeline;}else{p.render=pipeline;}
  }
  check_errors()?;Ok(p)
 }
 fn image(&mut self,key:(i64,usize),pose:[f32;3],scale:[f32;3],clip:[f32;4],kind:f32,id:f32,pressure:f32){
  if scale.iter().any(|v|!v.is_finite()||*v<=0.)||self.rows.len()/STRIDE>=MAX{return;}
  let Some(a)=self.art.get(&key).cloned().or_else(||self.art.iter().find(|((root,_),_)|*root==key.0).map(|(_,a)|a.clone()))else{self.missing.insert(key);return;};
  let low=std::array::from_fn::<_,3,_>(|i|pose[i]+a.lo[i]*scale[i]);let high=std::array::from_fn::<_,3,_>(|i|pose[i]+a.hi[i]*scale[i]);
  if high[0]<clip[0]||high[1]<clip[1]||low[0]>clip[2]||low[1]>clip[3]{return;}
  self.rows.extend_from_slice(&[
   [(low[0]+high[0])*0.5,(low[1]+high[1])*0.5,(high[0]-low[0])*0.5,(high[1]-low[1])*0.5],
   [pose[2],scale[2],a.offset as f32,a.edge as f32],clip,[kind,id,pressure,0.],
   [a.lo[0],a.lo[1],a.hi[0],a.hi[1]],[pose[0],pose[1],scale[0],scale[1]],[0.;4],[0.;4]]);
 }
 fn glyph(&mut self,position:[f32;3],font:f32,glyph:f32,clip:[f32;4]){
  if font<=0.||!(0. ..95.).contains(&glyph)||self.rows.len()/STRIDE>=MAX{return;}
  self.rows.extend_from_slice(&[[position[0]+0.48*font,position[1]+0.36*font,0.66*font,0.68*font],
   [position[2],font,glyph,0.],clip,[5.,0.,0.,0.],[0.;4],[position[0],position[1],font,font],[0.;4],[0.;4]]);
 }
 fn text(&mut self,f:&[[f32;4]],base:usize,max:usize,pose:[f32;3],font:f32,clip:[f32;4]){
  let codes:Vec<f32>=(0..max).map(|i|r(f,base+i/4)[i%4]).take_while(|c|*c>=32.).collect();
  for(i,c)in codes.iter().enumerate(){self.glyph([pose[0]+(i as f32-codes.len()as f32*0.5)*0.55*font,pose[1]-0.28*font,pose[2]],font,*c-32.,clip);}
 }
 pub(super) unsafe fn update(&mut self,queue:WGPUQueue,f:&[[f32;4]])->Result<(),String>{
  if f.len()!=super::super::workstation::FRAME_ROWS{return Ok(());}
  // Plain also owns the standalone studio background, where no world is drawn.
  self.plain_screen=r(f,9)[2]<0.5;
  self.rows.clear();let unclip=[-1e10,-1e10,1e10,1e10];
  // All runtime changes below are transforms, clipping and live display data.
  // No cache key, recapture request, optical plan, or settle generation exists.
  for i in 0..4{let b=32+i*40;let c=r(f,b+2);let h=r(f,b+3);if r(f,b)[0]<=0.||h[0]<=0.||h[1]<=0.{continue;}
   if let Some(a)=self.art.get(&(-(i as i64)-1,b)).cloned(){let scale=[2.*h[0]/(a.hi[0]-a.lo[0]),2.*h[1]/(a.hi[1]-a.lo[1]),2.*h[2]/(a.hi[2]-a.lo[2])];let pose=std::array::from_fn(|j|c[j]-(a.lo[j]+a.hi[j])*0.5*scale[j]);self.image((-(i as i64)-1,b),pose,scale,unclip,0.,i as f32,if i==2{r(f,116)[3]}else{0.});}}
  // Reuse the persistent bottom-panel image as an independently transformed
  // preamp gem. This never runs lighting or asks the offline baker to refresh.
  let pre=r(f,r(f,232)[0]as usize+60);
  if pre[2]>0.&&pre[3]>0. {let c=r(f,74);let h=r(f,75);let key=(-2,72);
   if let Some(a)=self.art.get(&key).cloned(){let scale=[2.*pre[2]/(a.hi[0]-a.lo[0]),2.*pre[3]/(a.hi[1]-a.lo[1]),2.*h[2]/(a.hi[2]-a.lo[2])];
    let center=[pre[0],pre[1],c[2]];let pose=std::array::from_fn(|j|center[j]-(a.lo[j]+a.hi[j])*0.5*scale[j]);self.image(key,pose,scale,unclip,0.,4.,0.);}}
  let ordinary=r(f,12)[0]as usize;let nav=r(f,232)[2]as usize;let meters=r(f,247)[1]as usize;let io=r(f,44288)[1]as usize;let keyboard=r(f,44592)[1]as usize;
  for i in 0..132{
   let active=if i>=125{i-125<keyboard.min(7)}else if i>=65{i-65<io.min(60)}else if i>=61{i-61<nav.min(4)}else if i>=29{i-29<meters.min(32)}else{i<ordinary.min(29)};if !active{continue;}
   let at=control_row(f,i);let p=r(f,at);if p[3]<=0.{continue;}let tree=r(f,at+1);let mat=if tree[3]>0.5{tree[3]as usize}else{r(f,12)[1]as usize};
   let clip=if (13..29).contains(&i)&&r(f,r(f,232)[0]as usize+44+i-13)[2]>r(f,r(f,232)[0]as usize+44+i-13)[0]{r(f,r(f,232)[0]as usize+44+i-13)}else{unclip};
   self.image((tree[0]as i64,mat),[p[0],p[1],p[2]],[p[3];3],clip,1.,(4+i*256)as f32,if i<4{r(f,45730)[i]}else if (13..29).contains(&i){tree[2]}else{0.});
   if i>=65 {
    let mut font=tree[2]*p[3];let mut y=p[1];
    {
     let letters=(0..12).map(|n|r(f,at+2+n/4)[n%4]).take_while(|c|*c>=32.).count().max(1) as f32;
     if let Some(a)=self.art.get(&(tree[0]as i64,mat)){font=(13.*r(f,r(f,255)[0]as usize+2)[0]).min((a.hi[0]-a.lo[0])*p[3]/(letters*0.55));y+=(a.hi[1]-a.lo[1])*p[3]*0.5+font*0.85;}
    }
    let before=self.rows.len();self.text(f,at+2,12,[p[0],y,p[2]+0.035*p[3]],font,unclip);
    for row in (before..self.rows.len()).step_by(STRIDE){self.rows[row+3][1]=1.;}
   }
   else if i>=61{self.glyph([p[0]-0.275*0.014*p[3],p[1]-0.28*0.014*p[3],p[2]+0.001*p[3]],0.014*p[3],tree[2]-32.,unclip);}
  }
  // Mixer components are independent images, including the moving fader cap.
  for i in 0..(r(f,43152)[0]as usize).min(70){let at=43160+i*6;let clip=r(f,at+4);let pose=r(f,at+3);
   for part in 0..3{let rect=r(f,at+part);if rect[2]<=0.||rect[3]<=0.{continue;}let root=if part==0{r(f,43153)[0]}else if part==1{r(f,43153)[2]}else{r(f,43154)[0]}as i64;
    let nominal=if part==0{[0.04,0.18]}else if part==1{[0.025,0.025]}else{[0.01,0.156]};
    let scale=[rect[2]/nominal[0],rect[3]/nominal[1],pose[1]];let low=if part==2{r(f,43154)[3]}else{r(f,43152)[2]};
    let pos=[rect[0],rect[1]-if part==1{r(f,43154)[2]*scale[1]}else{0.},pose[0]-low*scale[2]];
    let first=self.rows.len();
    self.image((root,12544),pos,scale,clip,3.,(800000+(i*3+part)*256)as f32,0.);
    if part==2&&self.rows.len()>first{
     // LED extent owns the height. Compress only the baked end rims from
     // 12 mm to 2 mm; keep the aperture and live spectrum transform intact.
     self.rows[first][3]=0.080*scale[1];
    }
   }
  }
  // Timeline copies use their own bounded packet; bottom-panel visibility and
  // horizontal scrolling cannot hide them or consume their record capacity.
  let lane=crate::workstation::timeline_controls::BASE;
  for i in 0..(r(f,lane)[0]as usize).min(32){let at=lane+8+i*6;let clip=r(f,at+4);let pose=r(f,at+3);
   for part in 0..3{let rect=r(f,at+part);if rect[2]<=0.||rect[3]<=0.{continue;}
    let root=if part==0{r(f,lane+1)[0]}else if part==1{r(f,lane+1)[2]}else{r(f,lane+2)[0]}as i64;
    let nominal=if part==0{[0.04,0.18]}else if part==1{[0.025,0.025]}else{[0.01,0.156]};
    let scale=[rect[2]/nominal[0],rect[3]/nominal[1],pose[1]];let low=if part==2{r(f,lane+2)[3]}else{r(f,lane)[2]};
    let pos=[rect[0],rect[1]-if part==1{r(f,lane+2)[2]*scale[1]}else{0.},pose[0]-low*scale[2]];
    let first=self.rows.len();self.image((root,12544),pos,scale,clip,if part==2{8.}else{3.},0.,0.);
    if part==2&&self.rows.len()>first{self.rows[first][3]=0.080*scale[1];self.rows[first+3][2]=pose[2];self.rows[first+3][3]=pose[3];}
   }
  }
  let lane_io=crate::workstation::timeline_controls::IO;
  for i in 0..(r(f,lane_io)[1]as usize).min(32){let at=lane_io+4+i*5;let p=r(f,at);let tree=r(f,at+1);
   self.image((tree[0]as i64,tree[3]as usize),[p[0],p[1],p[2]],[p[3];3],r(f,lane_io+3),1.,0.,0.);
  }
  // Timeline objects own images, so clip/zoom/pan never touches panel artwork.
  let drum_workspace=r(f,crate::workstation::piano_scene::BASE)[3]>0.5;
  let drum_start=self.rows.len();
  let header=r(f,248);let bottom=r(f,255);
  for i in 0..(header[0]as usize).min(256)+if drum_workspace{0}else{(bottom[2]as usize).min(12)}{
   let (instance,at)=if i<(header[0]as usize).min(256){(i,header[1]as usize+i*3)}else{let j=i-(header[0]as usize).min(256);(192+j,bottom[0]as usize+12+j*3)};
   let p=r(f,at);let tree=r(f,at+1);let size=r(f,at+2);let selected=if instance<192 && (tree[3]== -2. || tree[3]==2.){1.}else{0.};if p[3]<=0.{continue;}let clip=if instance>=192{r(f,bottom[0]as usize)}else{r(f,249)};
   if instance>=192 && r(f,bottom[0]as usize+2)[1]==3. {
    // Bottom controls reuse the transport mini-button artwork. Disabled controls
    // keep readable labels; active tabs/toggles select the white ruby image.
    let key=(2911427,if tree[2]==12704.{12704}else{2911387});
    if let Some(a)=self.art.get(&key).cloned(){
     let scale=[size[0]*p[3]/(a.hi[0]-a.lo[0]),size[1]*p[3]/(a.hi[1]-a.lo[1]),size[1]*p[3]/(a.hi[1]-a.lo[1])];
     let pos=[p[0]-(a.lo[0]+a.hi[0])*0.5*scale[0],p[1]-(a.lo[1]+a.hi[1])*0.5*scale[1],p[2]-a.hi[2]*scale[2]];
     self.image(key,pos,scale,clip,1.,(700000+instance*16)as f32,0.);
    }
   }else if tree[3]>0.5 {
    let tape_clip=[clip[0].max(p[0]),clip[1].max(p[1]-size[1]*p[3]*0.5),clip[2].min(p[0]+size[0]*p[3]),clip[3].min(p[1]+size[1]*p[3]*0.5)];
    for tile in 0..((size[0]+size[3])/0.22).ceil().max(1.).min(128.)as usize{self.image((tree[0]as i64,tree[2]as usize),[p[0]+(tile as f32*0.22-size[3])*p[3],p[1],p[2]],[p[3];3],tape_clip,2.,(700000+instance*16)as f32,selected);}
   }else if tree[3]>= -0.5{self.image((tree[0]as i64,tree[2]as usize),[p[0],p[1],p[2]],[p[3];3],clip,2.,(700000+instance*16)as f32,0.);}else{
    // Parametric tape uses the same baked tape image, stretched in XY.
    let key=if tree[2]==12704.{(-7,12704)}else{(-1,32)};
    if let Some(a)=self.art.get(&key).cloned(){let scale=[2.*size[0]*p[3]/(a.hi[0]-a.lo[0]),2.*size[1]*p[3]/(a.hi[1]-a.lo[1]),1.15*p[3]/(a.hi[2]-a.lo[2])];let pos=[p[0]-(a.lo[0]+a.hi[0])*0.5*scale[0],p[1]-(a.lo[1]+a.hi[1])*0.5*scale[1],p[2]+0.3*p[3]-a.hi[2]*scale[2]];self.image(key,pos,scale,clip,4.,(700000+instance*16)as f32,selected);}
   }
   let letters=if instance>=192{bottom[0]as usize+48+(instance-192)*4}else{header[2]as usize+instance*2};
   let mut font=size[3];let mut y=p[1];let before=self.rows.len();
   if instance>=192{let count=(0..16).map(|n|r(f,letters+n/4)[n%4]).take_while(|c|*c>=32.).count().max(1) as f32;font=(13.*r(f,bottom[0]as usize+2)[0]).min(size[0]*p[3]/(count*0.55));y+=size[1]*p[3]*0.5+font*0.85;}
   self.text(f,letters,if instance>=192{16}else{8},[p[0],y,p[2]+0.001],font,clip);
   if instance>=192{for row in (before..self.rows.len()).step_by(STRIDE){self.rows[row+3][1]=1.;}}
  }
  let drum_end=self.rows.len();
  // Rulers use fixed ruby art; only rectangles and depth vary.

  // Waveform zoom/loop bars share the timeline ruler's persistent ruby art.
  // Range edits only move/clip these images; no lighting work is performed.
  let base=bottom[0]as usize;
  if bottom[1]>0.5 && r(f,base+11)[0]>0.5 {
   let v=r(f,base+1);let mpp=r(f,base+2)[0];let range=r(f,base+10);
   let left=v[0]-8.*mpp;let right=v[2]+8.*mpp;let top=v[3]+8.*mpp;
   for (i,lo,hi) in [(0,range[0],range[1]),(1,((range[2]-range[0])/(range[1]-range[0]).max(1e-12)).clamp(0.,1.),((range[3]-range[0])/(range[1]-range[0]).max(1e-12)).clamp(0.,1.))] {
    // Full-length track and selected range, as on the timeline.
    for (a0,a1,h) in [(0.,1.,18.),(lo,hi,12.)] {
     if a1<=a0 {continue;}let key=(-1,32);
     if let Some(a)=self.art.get(&key).cloned(){let x0=left+a0*(right-left);let x1=left+a1*(right-left);let y=top+(if i==0{35.}else{13.})*mpp;
      let scale=[(x1-x0)/(a.hi[0]-a.lo[0]),h*mpp/(a.hi[1]-a.lo[1]),mpp];
      let z=r(f,base+2)[3]+if h==12.{0.002}else{0.001};
      let pos=[(x0+x1)*0.5-(a.lo[0]+a.hi[0])*0.5*scale[0],y-(a.lo[1]+a.hi[1])*0.5*scale[1],z-a.hi[2]*scale[2]];
      self.image(key,pos,scale,r(f,base),4.,0.,0.);
     }
    }
   }
  }
  // Actual Cycles source-material frames: placement and film clocks are
  // separate. Pixel alpha transmits the retained world, never a fixed matte.
  let bubble=r(f,234);let addresses=r(f,237);let clip=r(f,235);let mut pitch=0;
  for i in 0..(bubble[0]as usize).min(2048){let note=r(f,addresses[0]as usize+i);let mut height=bubble[1];
   while pitch<127 {let low=r(f,addresses[1]as usize+pitch*2);let high=r(f,addresses[1]as usize+pitch*2+1);if i>=low[3]as usize&&i<(low[3]+high[3])as usize{break;}pitch+=1;}
   if addresses[3]>1.5{let low=r(f,addresses[1]as usize+pitch*2);let high=r(f,addresses[1]as usize+pitch*2+1);height=(high[1]-low[1])/1.2;}
   if note[2]<=0.||height<=0.||self.rows.len()/STRIDE>=MAX{continue;}
   let (age,selected)=bubble_art::placement_age(note[3]);
   let family=self.bubble_bank.family(note[2],height);let art=&self.bubble_bank.families[family];let scale=height/0.018;
   let bend=r(f,crate::workstation::bubble_notes::BEND_BASE+i);
   let bend_end=bend.iter().sum::<f32>();
   let low=[note[0]-art.pad*scale,note[1]+art.low_y*scale+bend[0].min(bend_end)];let high=[note[0]+note[2]+art.pad*scale,note[1]+art.high_y*scale+bend[0].max(bend_end)];
   if high[0]<clip[0]||high[1]<clip[1]||low[0]>clip[2]||low[1]>clip[3]{continue;}
   let z=bubble[3]+if selected{height*0.15}else{0.};
   self.rows.extend_from_slice(&[
    [(low[0]+high[0])*0.5,(low[1]+high[1])*0.5,(high[0]-low[0])*0.5,(high[1]-low[1])*0.5],
    [z,scale,family as f32,age],clip,[7.,(crate::workstation::bubble_notes::BEND_GPU_BASE+i)as f32,if selected{1.}else{0.},0.],
    [art.length,art.pad,art.low_y,art.high_y],[note[0],note[1],note[2],height],
    r(f,crate::workstation::bubble_notes::IDENTITY_BASE+i),[pitch as f32,0.,0.,0.]]);
  }
  let spheres=r(f,240);let clip=r(f,241);
  for i in 0..(spheres[0]as usize).min(64){let at=spheres[1]as usize+i*spheres[2]as usize;let p=r(f,at);let radii=r(f,at+1);let before=self.rows.len();self.image((-6,0),[p[0],p[1],p[2]],[radii[0],radii[1],radii[2]],clip,6.,0.,0.);if self.rows.len()>before{self.rows[before+6]=r(f,at+2);}}
  // Settings-page buttons use the same line bounds as the engraved labels.
  // Reuse persistent mini-ruby artwork; this adds no lighting or baking work.
  let lines=r(f,222);
  for i in 0..(lines[1]as usize).min(64){
   let at=lines[0]as usize+i*3;let center=r(f,at);let half=r(f,at+1);let info=r(f,at+2);
   if info[0]!=3.{continue;}
   let key=(2911427,2911387);
   if let Some(a)=self.art.get(&key).cloned(){
    let pad=info[1]*(6./21.6);let width=2.*(half[0]+pad);let height=2.*(half[1]+pad);
    let scale=[width/(a.hi[0]-a.lo[0]),height/(a.hi[1]-a.lo[1]),height/(a.hi[1]-a.lo[1])];
    let c=r(f,154);let h=r(f,155);let z=c[2]+h[2];
    let pose=[center[0]-(a.lo[0]+a.hi[0])*0.5*scale[0],center[1]-(a.lo[1]+a.hi[1])*0.5*scale[1],z-a.hi[2]*scale[2]];
    self.image(key,pose,scale,unclip,1.,0.,0.);
   }
  }
  // Browser glyphs are separate sprites; moving/changing a word never bakes a gem.
  let count=(r(f,212)[2]as usize).min(1536);let gpu=r(f,212)[1]as usize;let base=if gpu==crate::workstation::piano_scene::GPU_END{crate::workstation::piano_scene::END}else{gpu};
  for i in 0..count{let p=r(f,base+i*2);let d=r(f,base+i*2+1);let panel=d[1]as usize;if panel>3{continue;}let c=r(f,34+40*panel);let h=r(f,35+40*panel);let clip=[c[0]-h[0],c[1]-h[1],c[0]+h[0],c[1]+h[1]];let before=self.rows.len();self.glyph([p[0],p[1],c[2]+h[2]+0.0001],p[3],d[0],clip);
   // Compact/right panel: brighter, heavier engraved labels at the same size.
   if (panel==2 || panel==3) && self.rows.len()>before{self.rows[before+3][1]=1.;}
  }
  // The piano editor owns its full-screen presentation just as in the
  // original application. Do not leave DAW panel/control images over its keys.
  if r(f,crate::workstation::piano_scene::BASE)[0]>0.5{
   self.rows=self.rows.chunks_exact(STRIDE).filter(|sprite|sprite[3][0]==7.).flat_map(|sprite|sprite.iter().copied()).collect();
   // Piano toolbar reuses the transport's immutable mini ruby/colorless-ruby art.
   // Only placement and image selection change; never refresh lighting.
   let base=crate::workstation::piano_scene::BASE;
   let room=r(f,base);let mpp=1.3/room[2];let selected=r(f,base+6)[0]as usize;
   let count=(r(f,base+8)[0]as usize).min(18);
   for i in 0..count {
    let bounds=r(f,base+1980+i);let width=bounds[2];let height=bounds[3];
    let latched=i==selected;let key=(2911427,if latched{12704}else{2911387});
    let center=[(bounds[0]+width*0.5-room[1]*0.5)*mpp,(room[2]*0.5-bounds[1]-height*0.5)*mpp];
    let z=if latched{0.009}else{0.012};
    if let Some(a)=self.art.get(&key).cloned(){
     let scale=[width*mpp/(a.hi[0]-a.lo[0]),height*mpp/(a.hi[1]-a.lo[1]),height*mpp/(a.hi[1]-a.lo[1])];
     let pose=[center[0]-(a.lo[0]+a.hi[0])*0.5*scale[0],center[1]-(a.lo[1]+a.hi[1])*0.5*scale[1],z-a.hi[2]*scale[2]];
     self.image(key,pose,scale,unclip,1.,0.,0.);
    }
    for row in [i,count+i] {
     let text=base+512+row*18;let label=r(f,text);
     for j in 0..label[3]as usize {
      let code=r(f,text+2+j/4)[j%4];let at=self.rows.len();
      self.glyph([label[0]+j as f32*0.55*label[2],label[1],z+0.001],label[2],code-32.,unclip);
      if self.rows.len()>at{self.rows[at+3][1]=1.;}
     }
    }
   }

  }
  for i in 0..3{let p=r(f,45744+i);let size=r(f,45747+i);if size[3]<=0.||p[3]<=0.{continue;}let key=(-1,32);if let Some(a)=self.art.get(&key).cloned(){let scale=[2.*size[0]*p[3]/(a.hi[0]-a.lo[0]),2.*size[1]*p[3]/(a.hi[1]-a.lo[1]),p[3]];let pos=[p[0]-(a.lo[0]+a.hi[0])*0.5*scale[0],p[1]-(a.lo[1]+a.hi[1])*0.5*scale[1],p[2]-a.hi[2]*scale[2]];self.image(key,pos,scale,unclip,4.,i as f32,0.);}}
  if drum_workspace {
   // Keep only the rack's controls and the shared baked bubble images.
   // This is image selection, never runtime optical work or a panel backdrop.
   self.rows=self.rows.chunks_exact(STRIDE).enumerate()
    .filter(|(i,sprite)| (*i*STRIDE>=drum_start && *i*STRIDE<drum_end) || sprite[3][0]==7.)
    .flat_map(|(_,sprite)|sprite.iter().copied()).collect();
  }
  self.animations.clear();let now=self.clock();let mut births=BTreeMap::new();
  for row in (0..self.rows.len()).step_by(STRIDE){
   if self.rows[row+3][0]!=7.||self.theme.is_some(){continue;}
   let words=self.rows[row+6];let mut id=0u64;
   for(i,word)in words.iter().enumerate(){id|=((*word as u64)&0xffff)<<(i*16);}
   if id==0{id=(1u64<<63)|(row/STRIDE+1)as u64;}
   let age=self.rows[row+1][3]as f64;
   let born=if age<1.35{Some(now-age)}else{self.births.remove(&id).flatten()};
   births.insert(id,born);
   self.animations.push(Animation{row,family:self.rows[row+1][2]as usize,age,updated:now,born,phase:(id%1_000_003)as f64/1_000_003.*10.});
  }
  self.births=births;self.frame_rows(now);
  if self.rows.iter().flatten().any(|v|!v.is_finite()){return Err("nonfinite baked DAW sprite transform".into());}
  self.count=(self.rows.len()/STRIDE)as u32;self.uploads+=1;
  let projection=if r(f,9)[0]<0.{[r(f,16),r(f,17),r(f,18)]}else{[[0.;4];3]};
  (self.api.wgpuQueueWriteBuffer)(queue,self.metadata,(MAX*STRIDE*16)as u64,projection.as_ptr().cast(),48);
  if let Some(theme)=&self.theme {
   let mut styles=Vec::with_capacity(self.count as usize*8);
   for sprite in self.rows.chunks_exact(STRIDE){styles.extend_from_slice(&cardinal_style(theme,self.cardinal_atlas.as_ref(),sprite)?);}
   (self.api.wgpuQueueWriteBuffer)(queue,self.metadata,(THEME_BASE*16)as u64,styles.as_ptr().cast(),styles.len()*16);
   let mut globals=vec![theme.color("background",[0.,0.5,0.5,1.]),theme.color("piano_roll",[0.,0.12,0.12,1.]),theme.color("piano_roll_accent",[0.,0.2,0.2,1.]),theme.surface("timeline_grid",[1.;4])];
   for family in 0..3{for i in 0..6{globals.push(theme.meter(family,i));}}
   globals.push([theme.document["background_grain"].as_f64().unwrap_or(0.)as f32,0.,0.,0.]);
   (self.api.wgpuQueueWriteBuffer)(queue,self.metadata,((THEME_BASE+MAX*8)*16)as u64,globals.as_ptr().cast(),globals.len()*16);
  }
  if !self.rows.is_empty(){(self.api.wgpuQueueWriteBuffer)(queue,self.metadata,0,self.rows.as_ptr().cast(),self.rows.len()*16);}Ok(())
 }
 fn clock(&self)->f64{self.fixed_time.unwrap_or_else(||self.started.elapsed().as_secs_f64())}
 fn frame_rows(&mut self,now:f64){
  for animation in &self.animations{
   let age=animation.age+(now-animation.updated).max(0.);
   let clock=self.fixed_time.map_or_else(||animation.born.map_or(now+animation.phase,|born|(now-born-1.35).max(0.)),|time|time+animation.phase);
   let (images,weights)=self.bubble_bank.frames(animation.family,age,clock);
   self.rows[animation.row+6]=images;self.rows[animation.row+7]=weights;
  }
 }
 pub(super) fn needs_redraw(&self)->bool{!self.animations.is_empty()&&self.fixed_time.is_none()&&self.last_animation.elapsed().as_secs_f64()>=1./60.}
 pub(super) unsafe fn animate(&mut self,queue:WGPUQueue){
  if self.animations.is_empty(){return;}
  self.frame_rows(self.clock());self.last_animation=std::time::Instant::now();
  (self.api.wgpuQueueWriteBuffer)(queue,self.metadata,0,self.rows.as_ptr().cast(),self.rows.len()*16);self.animation_uploads+=1;
 }
 pub(super) unsafe fn draw(&self,pass:WGPURenderPassEncoder,fullscreen_first:u32){let a=self.api;
  if self.plain_screen{(a.wgpuRenderPassEncoderSetPipeline)(pass,self.backdrop);(a.wgpuRenderPassEncoderSetBindGroup)(pass,0,self.group,0,ptr::null());(a.wgpuRenderPassEncoderDraw)(pass,6,1,fullscreen_first,0);}
  (a.wgpuRenderPassEncoderSetPipeline)(pass,self.render);(a.wgpuRenderPassEncoderSetBindGroup)(pass,0,self.group,0,ptr::null());(a.wgpuRenderPassEncoderDraw)(pass,6,self.count,0,0);
  (a.wgpuRenderPassEncoderSetPipeline)(pass,self.overlay);(a.wgpuRenderPassEncoderDraw)(pass,6,1,fullscreen_first,0);self.draws.set(self.draws.get()+1);
 }
 pub(super) fn info(&self)->Value{json!({"enabled":true,"policy":"offline immutable lighting; independent image composition","theme":self.theme.as_ref().map(|t|t.summary()).unwrap_or(json!({"name":"3D gem ruby","main":true})),"disk_image_bytes":self.bytes,"gpu_image_buffer_bytes":self.image_buffer_bytes,"gpu_auxiliary_image_buffer_bytes":self.auxiliary_buffer_bytes,"art_variants":self.art.len(),"sprite_count":self.count,"transform_uploads":self.uploads,"draws":self.draws.get(),"bubble_image_bytes":self.bubble_bank.bytes,"animated_bubble_count":self.animations.len(),"bubble_frame_uploads":self.animation_uploads,"runtime_optical_bakes":0,"runtime_optical_resolves":0,"missing_art":self.missing.iter().collect::<Vec<_>>()})}
}
impl Drop for Pipeline{fn drop(&mut self){unsafe{let a=self.api;for p in [self.render,self.overlay,self.backdrop]{if !p.is_null(){(a.wgpuRenderPipelineRelease)(p);}}if !self.group.is_null(){(a.wgpuBindGroupRelease)(self.group);}if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}for b in [self.images,self.metadata,self.bubble_images]{if !b.is_null(){(a.wgpuBufferRelease)(b);}}}}}

#[cfg(test)]mod tests{
 use super::*;
 fn called(block:&naga::Block,out:&mut Vec<naga::Handle<naga::Function>>){for statement in block.iter(){match statement{
  naga::Statement::Call{function,..}=>out.push(*function),
  naga::Statement::Block(b)=>called(b,out),
  naga::Statement::If{accept,reject,..}=>{called(accept,out);called(reject,out);},
  naga::Statement::Switch{cases,..}=>for c in cases{called(&c.body,out);},
  naga::Statement::Loop{body,continuing,..}=>{called(body,out);called(continuing,out);},_=>{}
 }}}
 #[test]fn interactive_sprite_entries_cannot_reach_optical_lighting(){
  let source=source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
  let module=naga::front::wgsl::parse_str(&source).unwrap_or_else(|e|panic!("{}",e.emit_to_string(&source)));
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
  for name in ["vs_baked","fs_baked","fs_baked_overlay","fs_baked_backdrop"]{
   let entry=module.entry_points.iter().find(|e|e.name==name).unwrap();let mut pending=vec![];called(&entry.function.body,&mut pending);let mut seen=std::collections::HashSet::new();
   while let Some(h)=pending.pop(){if !seen.insert(h){continue;}let f=&module.functions[h];let name=f.name.as_deref().unwrap_or("");
    assert!(!["gemTrace","gemNearest","gemAny","gemOpaque","gemOpaqueSurface","gemFresnel","gemVisibility","gemShadow","gemFilmReflectance","gemAudioSphereShade"].contains(&name),"live lighting reachable: {name}");called(&f.body,&mut pending);
   }
  }
 }
 #[test]fn cardinal_2d_shader_and_transport_targets_are_valid(){
  let s=source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap().replace("const CARDINAL_2D:bool=false;","const CARDINAL_2D:bool=true;");
  let m=naga::front::wgsl::parse_str(&s).unwrap_or_else(|e|panic!("{}",e.emit_to_string(&s)));
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();
  let mut d:Value=serde_json::from_str(include_str!("../Art/AudioWorkstation/CardinalThemes/Default.json")).unwrap();
  d["transport_play_button"]=json!("123456FF");let t=crate::workstation::cardinal_theme::Theme::parse(d).unwrap();
  let mut sprite=[[0.;4];8];sprite[3]=[1.,772.,0.,0.];let style=cardinal_style(&t,None,&sprite).unwrap();
  assert_eq!(style[0],crate::workstation::cardinal_theme::color_hex("123456FF").unwrap());assert_eq!(style[6][0],3.);
 }
 #[test]fn baked_art_is_complete_bounded_and_saved_on_disk(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("Art/AudioWorkstation/Baked");let d:Value=serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();let size=std::fs::metadata(root.join("images.rgba32f")).unwrap().len()as usize;
  let entries=d["entries"].as_array().unwrap();assert!(entries.len()>=23);assert!(size>0&&size<=128*1024*1024);
  for e in entries{let offset=number(e,"offset").unwrap();let edge=number(e,"edge").unwrap();assert!(offset+edge*edge*2<=size/16);}
 }
 #[test]fn panel_cut_corners_are_transparent_and_centers_covered(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("Art/AudioWorkstation/Baked");
  let manifest:Value=serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
  let pixels=std::fs::read(root.join("images.rgba32f")).unwrap();
  let value=|row:usize,c:usize|f32::from_le_bytes(pixels[row*16+c*4..row*16+c*4+4].try_into().unwrap());
  let mut panels=0;
  for e in manifest["entries"].as_array().unwrap().iter().filter(|e|e["kind"]==0){
   panels+=1;let edge=number(e,"edge").unwrap();let offset=number(e,"offset").unwrap();
   for(x,y)in [(0,0),(edge-1,0),(0,edge-1),(edge-1,edge-1)]{
    for row in [offset+y*edge*2+x,offset+y*edge*2+edge+x]{
     for c in 0..4{assert_eq!(value(row,c),0.,"opaque miss in {} corner ({x},{y})",e["name"]);}
    }
   }
   let mut misses=0;
   for y in 0..edge{for x in 0..edge{
    let color=offset+y*edge*2+x;let aux=color+edge;
    if value(aux,3)==0.{misses+=1;for c in 0..4{assert_eq!(value(color,c),0.,"miss radiance/matte must be zero");}}
    else{assert_eq!(value(aux,3),1.);assert!((e["min"][2].as_f64().unwrap()as f32-0.001..=e["max"][2].as_f64().unwrap()as f32+0.001).contains(&value(aux,0)),"invalid hit distance baked as coverage");}
   }}
   assert!(misses>0&&misses<edge*edge/2,"faceted silhouette must retain transparent cut corners and solid interior");
   let center=offset+(edge/2)*edge*2+edge+edge/2;
   assert_eq!(value(center,3),1.,"panel interior must remain covered");
   assert!(value(center,0)>=e["min"][2].as_f64().unwrap()as f32-0.001);
   assert!(value(center,0)<=e["max"][2].as_f64().unwrap()as f32+0.001);
  }
  assert_eq!(panels,5);
 }
 #[test]#[ignore="explicit offline artwork metadata export"]
 fn export_stock_art_definitions(){
  let kit=crate::workstation::stock_eq_gems::Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
  let variants:Vec<_>=kit.variants.iter().enumerate().map(|(i,v)|json!({"name":format!("StockEQ{i}"),"root":v.root,"end":v.end,"min":v.min,"max":v.max,"material":v.base})).collect();
  let path=std::env::var_os("WORMWOOD_BAKED_DEFINITIONS").expect("explicit metadata destination");std::fs::write(path,serde_json::to_vec_pretty(&variants).unwrap()).unwrap();
 }
}

// Map rendering roles to Cardinal's stable theme target names. Keep this
// translation here: no Cardinal System, timeline model or audio code is imported.
fn cardinal_style(t:&crate::workstation::cardinal_theme::Theme,atlas:Option<&cardinal_atlas::Atlas>,s:&[[f32;4]])->Result<[[f32;4];8],String>{
 let mode=s[3];let id=mode[1]as usize;
 let (kind,target,icon)=match mode[0]as i32{
  0=>("panel",["panel_top","panel_bottom","panel_left","panel_right","panel_bottom"][id.min(4)].to_owned(),-1.),
  1 if id>=4&&id<1028&&(id-4)%256==0=>("button",["transport_rewind","transport_stop","transport_record","transport_play"][(id-4)/256].to_owned(),((id-4)/256)as f32),
  1=>("button",format!("control:{id}"),-1.),
  2=>("surface",format!("clip:midi:{}",id.saturating_sub(700000)/16),-1.),
  3|8=>{let part=id.saturating_sub(800000)/256%3;("surface",format!("{}:audio:{}",["fader_housing","fader_knob","meter"][part],id.saturating_sub(800000)/768),-1.)},
  4=>("surface",if id<3{["timeline_zoom","timeline_zoom","timeline_loop"][id].into()}else{format!("track:audio:{id}")},-1.),
  5=>("text","text".into(),-1.),
  7=>("surface","midi_note".into(),-1.),
  _=>("surface","fader_housing_default".into(),-1.)
 };
 let m=t.material(kind,&target);let mut color=m.color;
 if mode[0]==7.{color=t.palette("midi_pitch_palette",s[7][0]as usize%12,color);}
 let meter=mode[0]==8.||(mode[0]==3.&&id>=800000&&(id-800000)/256%3==2);
 let image=if m.gem{atlas.map(|a|a.image(&format!("{kind}:{target}"),(s[0][2]/s[0][3]).max(0.001))).unwrap_or([0.;4])}else{[0.;4]};
 if m.gem&&image[3]<0.5{return Err(format!("Cardinal crystal atlas is missing {kind}:{target}"));}
 Ok([color,m.a,m.b,m.gradient,m.effects,m.glyph,[icon,if m.gem{1.}else{0.},if meter{1.}else{0.},if m.jelly||mode[0]==7.{1.}else{0.}],image])
}

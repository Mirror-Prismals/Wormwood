//! Finite benchmark-only changed-output diagnostics. Never a render default.
use super::*;
#[path="WorkstationCaptureAttributionSource.rs"]mod source;
fn selected(mode:Option<&str>,args:&[String])->Result<Option<String>,String>{
 let Some(mode)=mode.filter(|m|*m!="off"&&*m!="0")else{return Ok(None);};
 if !source::MODES.contains(&mode){return Err("unknown optical capture diagnostic".into());}
 let finite=args.windows(2).any(|p|p[0]=="--frames"&&p[1].parse::<u32>().is_ok_and(|n|(1..=600).contains(&n)));
 if !finite||!args.iter().any(|a|a=="--performance")||!args.iter().any(|a|a=="--benchmark-game-workstation"){
  return Err("Changed-output optical diagnostics require finite --performance --benchmark-game-workstation --frames1..600; never use for interactive testing".into());
 }
 Ok(Some(mode.to_owned()))
}
pub(super) struct Runtime{api:&'static Api,shader:WGPUShaderModule,pipeline:WGPUComputePipeline,mode:String,encoded:u64}
impl Runtime{
 pub(super) unsafe fn new(c:&Client,input:&str,layout:WGPUPipelineLayout,incompatible:bool)->Result<Option<Self>,String>{
  let env=std::env::var("WORMWOOD_WORKSTATION_CAPTURE_ABLATION").ok();let args:Vec<_>=std::env::args().collect();let Some(mode)=selected(env.as_deref(),&args)?else{return Ok(None);};
  if incompatible{return Err("Capture attribution must use original capture workgroups without split/plane-table experiments".into());}
  let text=source::transform(input,&mode)?;let a=c.api;
  let mut s=Self{api:a,shader:ptr::null_mut(),pipeline:ptr::null_mut(),mode,encoded:0};
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(&text)};
  s.shader=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("DIAGNOSTIC incomplete optical capture")});
  if s.shader.is_null(){return Err("diagnostic capture shader allocation failed".into());}check_errors()?;
  s.pipeline=(a.wgpuDeviceCreateComputePipeline)(c.device,&WGPUComputePipelineDescriptor{layout,compute:WGPUProgrammableStageDescriptor{module:s.shader,entryPoint:view("cs_retained"),..Default::default()},..Default::default()});
  if s.pipeline.is_null(){return Err("diagnostic capture pipeline allocation failed".into());}check_errors()?;Ok(Some(s))
 }
 pub(super) fn pipeline(&mut self)->WGPUComputePipeline{self.encoded+=1;self.pipeline}
 pub(super) fn info(&self)->Value{json!({"mode":self.mode,"encoded_captures":self.encoded,"changed_output":true,"scope":"Counterfactual synchronous capture ONLY. Original packets, response preparation, incremental stages and compositor remain. These deliberately incomplete images are not proposed graphics; cost deltas include compiler/secondary visibility effects and are not additive exclusive costs. Never report these rates as optimized app FPS."})}
}
impl Drop for Runtime{fn drop(&mut self){unsafe{if !self.pipeline.is_null(){(self.api.wgpuComputePipelineRelease)(self.pipeline);}if !self.shader.is_null(){(self.api.wgpuShaderModuleRelease)(self.shader);}}}}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn diagnostic_requires_bounded_explicit_combined_benchmark(){let a:Vec<String>=["engine","--performance","--benchmark-game-workstation","WebGPU","--frames","120"].iter().map(|s|s.to_string()).collect();for mode in source::MODES{assert_eq!(selected(Some(mode),&a).unwrap(),Some(mode.to_string()));assert!(selected(Some(mode),&[]).is_err());}for n in["0","601","forever"]{let mut b=a.clone();b[5]=n.into();assert!(selected(Some(source::MODES[0]),&b).is_err());}assert!(selected(Some("typo"),&a).is_err());assert_eq!(selected(None,&[]).unwrap(),None);assert_eq!(selected(Some("off"),&[]).unwrap(),None);}
 #[test]fn full_production_diagnostic_sources_validate_without_changing_capture_entry(){let raw=include_str!("../Procedures/Shaders/workstation.wgsl");let base=super::super::source::source(raw,true,false,true).unwrap();let ledger=super::super::source::with_meter_ledger(&base).unwrap();let tiled=super::super::incremental::tiled_source(&ledger).unwrap();for mode in source::MODES{assert!(source::transform(&tiled,mode).unwrap().contains("image intentionally differs"));}assert_eq!(source::transform(&tiled,"off").unwrap(),tiled);}
}

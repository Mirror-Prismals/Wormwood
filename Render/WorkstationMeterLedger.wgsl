// Production opt-in storage contract. State0=invalid/building,1=complete,
// 2=overflow,3=unsupported/nonfinite. Header count includes overflow terms.
struct MeterLedgerTerm {
 weight:vec3<f32>,selector:u32,
 off:vec3<f32>,threshold:f32,
 on:vec3<f32>,reserved:u32,
};
struct MeterLedgerSlot { header:vec4<u32>,terms:array<MeterLedgerTerm,16> };
struct MeterLedgerSlots { slots:array<MeterLedgerSlot> };
struct MeterLedgerJob { pixel:vec2<u32>,sample:u32,pad:u32 };
struct MeterLedgerJobs { jobs:array<MeterLedgerJob> };
struct MeterLedgerMap { pixels:array<u32> };
@group(0) @binding(9) var<storage,read_write> meterLedgerSlots:MeterLedgerSlots;
@group(0) @binding(10) var<storage,read> meterLedgerJobs:MeterLedgerJobs;
@group(0) @binding(13) var<storage,read> meterLedgerMap:MeterLedgerMap;
var<private> meterLedgerIndex:u32;
var<private> meterLedgerCount:u32;
var<private> meterLedgerOverflow:bool;
var<private> meterLedgerRejected:bool;
var<private> meterLedgerSelector:u32;
var<private> meterLedgerThreshold:f32;
fn meterLedgerFinite(v:vec3<f32>)->bool {
 return all((bitcast<vec3<u32>>(v)&vec3<u32>(0x7f800000u))!=vec3<u32>(0x7f800000u));
}
fn meterLedgerAppend(weight:vec3<f32>,off:vec3<f32>,on:vec3<f32>,selector:u32,threshold:f32){
 let index=meterLedgerCount;meterLedgerCount+=1u;
 if(index>=16u){meterLedgerOverflow=true;return;}
 if(!meterLedgerFinite(weight)||!meterLedgerFinite(off)||!meterLedgerFinite(on)){
  meterLedgerRejected=true;return;
 }
 meterLedgerSlots.slots[meterLedgerIndex].terms[index]=MeterLedgerTerm(weight,selector,off,threshold,on,0u);
}
fn meterLedgerRecordConstant(weight:vec3<f32>,radiance:vec3<f32>){
 meterLedgerAppend(weight,radiance,radiance,0xffffffffu,0.0);
}
fn meterLedgerRecordOpaque(weight:vec3<f32>,current:vec3<f32>,point:vec3<f32>,direction:vec3<f32>,normal:vec3<f32>,panel:i32,dynamicTerm:bool){
 if(!dynamicTerm){meterLedgerRecordConstant(weight,current);return;}
 if(meterLedgerCount>=16u){meterLedgerCount+=1u;meterLedgerOverflow=true;return;}
 meterLedgerSelector=0xffffffffu;meterLedgerThreshold=-1.0;
 let off=meterLedgerOpaqueState(point,direction,normal,panel,0.25);
 let selector=meterLedgerSelector;let threshold=meterLedgerThreshold;
 let on=meterLedgerOpaqueState(point,direction,normal,panel,1.0);
 if(selector==0xffffffffu||selector!=meterLedgerSelector||bitcast<u32>(threshold)!=bitcast<u32>(meterLedgerThreshold)
    ||!(threshold>=0.0&&threshold<=1.0)){meterLedgerRejected=true;}
 meterLedgerAppend(weight,off,on,selector,threshold);
}
// Alpha1 accepts the COMPLETE ordered sum. Alpha0 means canonical fallback.
fn meterLedgerEvaluate(index:u32)->vec4<f32>{
 if(index>=arrayLength(&meterLedgerSlots.slots)){return vec4<f32>(0.0);}
 let header=meterLedgerSlots.slots[index].header;
 if(header.x!=1u||header.y>16u){return vec4<f32>(0.0);}
 var result=vec3<f32>(0.0);
 for(var i:u32=0u;i<16u;i++){
  if(i>=header.y){break;}
  let term=meterLedgerSlots.slots[index].terms[i];var radiance=term.off;
  if(term.selector!=0xffffffffu){
   let row=term.selector/4u;let lane=term.selector%4u;
   if(row>=arrayLength(&workstationData.rows)){return vec4<f32>(0.0);}
   let level=gemRow(i32(row))[lane];
   if(!(level>=0.0&&level<=1.0)){return vec4<f32>(0.0);}
   if(term.threshold<clamp(level,0.0,1.0)){radiance=term.on;}
  }
  result+=term.weight*radiance;
 }
 return vec4<f32>(result,1.0);
}
// The full-screen map contains the sample0 slot index of each compact pixel;
// sample1 immediately follows. Unmapped, pending, overflow and bad identities
// fail closed. Publication/dependency validation is owned by the renderer.
fn meterLedgerLookup(pixel:vec2<f32>,sample:i32)->vec4<f32>{
 let resolution=gemRow(8).xy;
 if(sample<0||sample>=2||any(resolution<vec2<f32>(1.0))||any(resolution>vec2<f32>(16384.0))
    ||any(resolution!=floor(resolution))||any(pixel<vec2<f32>(0.0))
    ||any(pixel>=resolution)||any(pixel!=floor(pixel))){return vec4<f32>(0.0);}
 let xy=vec2<u32>(pixel);let address=xy.y*u32(resolution.x)+xy.x;
 if(address>=arrayLength(&meterLedgerMap.pixels)){return vec4<f32>(0.0);}
 let base=meterLedgerMap.pixels[address];
 if(base==0xffffffffu){return vec4<f32>(0.0);}
 let index=base+u32(sample);
 if(index>=arrayLength(&meterLedgerSlots.slots)){return vec4<f32>(0.0);}
 let header=meterLedgerSlots.slots[index].header;
 if(header.z!=address||header.w!=u32(sample)){return vec4<f32>(0.0);}
 return meterLedgerEvaluate(index);
}
// GPU-only capture. There is no diagnostic reference trace, output buffer,
// mode metadata or CPU readback. One invocation exclusively owns each slot.
@compute @workgroup_size(64,1,1)
fn cs_meter_ledger_capture(@builtin(global_invocation_id) id:vec3<u32>){
 if(id.y!=0u||id.z!=0u||id.x>=arrayLength(&meterLedgerJobs.jobs)
    ||id.x>=arrayLength(&meterLedgerSlots.slots)){return;}
 let index=id.x;let job=meterLedgerJobs.jobs[index];
 meterLedgerSlots.slots[index].header=vec4<u32>(0u);
 let resolution=gemRow(8).xy;
 if(job.sample>=2u||any(resolution<vec2<f32>(1.0))||any(resolution>vec2<f32>(16384.0))
    ||any(resolution!=floor(resolution))||any(job.pixel>=vec2<u32>(resolution))){return;}
 retainedActive=false;retainedPrimary=false;
 meterLedgerIndex=index;meterLedgerCount=0u;meterLedgerOverflow=false;meterLedgerRejected=false;
 let origin=retainedRay(vec2<f32>(job.pixel),i32(job.sample));let direction=gemSafeUnit(gemRow(4).xyz);
 let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(job.sample));
 let captured=meterLedgerCaptureTrace(origin,direction,random);
 var state=1u;if(meterLedgerRejected){state=3u;}if(meterLedgerOverflow){state=2u;}
 let address=job.pixel.y*u32(resolution.x)+job.pixel.x;
 meterLedgerSlots.slots[index].header=vec4<u32>(state,meterLedgerCount,address,job.sample);
}

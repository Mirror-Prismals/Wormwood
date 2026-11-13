//! Direct port of Salamander137 Host/DawMusicalTime.h, DawTimelineGrid.h and
//! DawMusicalTimeSerialization.h; SnapSample from DawRubatoTimelineSystem.cpp.
//! Project timing is independent of each existing clip's local MIDI clock.
use serde_json::{Value,json};
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct Signature {pub quarter:f64,pub numerator:i32,pub denominator:i32}
impl Default for Signature {fn default()->Self{Self{quarter:0.,numerator:4,denominator:4}}}
impl Signature {fn valid(self)->bool{self.quarter.is_finite()&&(0. ..=1e9).contains(&self.quarter)&&(1..=32).contains(&self.numerator)&&(1..=32).contains(&self.denominator)&&(self.denominator & (self.denominator-1))==0}}
#[derive(Clone,Debug,PartialEq)]
pub(crate) struct Region {pub start_quarter:f64,pub seconds:Vec<f64>,pub ripple:bool,pub length_quarters:f64}
impl Region {
 pub fn span(&self)->f64{if self.length_quarters>0.{self.length_quarters}else{self.seconds.len().saturating_sub(1)as f64}}
 pub fn end(&self)->f64{self.start_quarter+self.span()}
 pub fn quarter_at_index(&self,i:usize)->f64{self.start_quarter+(i as f64).min(self.span())}
 fn segment(&self,i:usize)->f64{(self.span()-i as f64).min(1.)}
 pub fn valid(&self)->bool{
  self.start_quarter.is_finite()&&(0. ..=1e9).contains(&self.start_quarter)
   &&(2..=65537).contains(&self.seconds.len())&&self.seconds[0]==0.
   &&self.length_quarters.is_finite()&&self.length_quarters>=0.&&self.span()>0.&&self.span()<=65536.
   &&self.span().ceil()==(self.seconds.len()-1)as f64
   &&self.seconds.windows(2).all(|a|a[1].is_finite()&&a[1]<=1e9&&a[1]-a[0]>=0.000001)
 }
 pub fn move_beat(&mut self,index:usize,seconds:f64)->bool{
  if !self.valid()||!seconds.is_finite()||index==0||index>=self.seconds.len()||(!self.ripple&&index+1==self.seconds.len()){return false;}
  let mut seconds=seconds.max(self.seconds[index-1]+0.001);
  if !self.ripple{seconds=seconds.min(self.seconds[index+1]-0.001);}
  let delta=seconds-self.seconds[index];
  if self.ripple{if self.seconds.last().unwrap()+delta>1e9{return false;}for value in &mut self.seconds[index..]{*value+=delta;}}
  else{self.seconds[index]=seconds;}true
 }
}
#[derive(Clone,Copy,Debug)]
pub(crate) struct Position {pub signature:Signature,pub signature_quarter:f64,pub bar:i64,pub beat:i32,pub beat_fraction:f64}
#[derive(Clone,Debug,PartialEq)]
pub(crate) struct Map {pub bpm:f64,pub sample_rate:f64,pub zero_sample:i64,pub signatures:Vec<Signature>,pub regions:Vec<Region>}
impl Default for Map {fn default()->Self{Self{bpm:120.,sample_rate:48000.,zero_sample:0,signatures:Vec::new(),regions:Vec::new()}}}
impl Map {
 pub fn seconds_per_quarter(&self)->f64{60./self.bpm.clamp(20.,400.)}
 pub fn seconds_at(&self,quarter:f64)->f64{
  let step=self.seconds_per_quarter();let mut shift=0.;
  for r in &self.regions{
   let start=r.start_quarter*step+shift;if quarter<r.start_quarter{break;}
   if quarter<r.end(){let relative=quarter-r.start_quarter;let i=relative.floor()as usize;return start+r.seconds[i]+(relative-i as f64)/r.segment(i)*(r.seconds[i+1]-r.seconds[i]);}
   shift+=r.seconds.last().unwrap()-(r.end()-r.start_quarter)*step;
  }quarter*step+shift
 }
 pub fn quarter_at_seconds(&self,seconds:f64)->f64{
  let step=self.seconds_per_quarter();let mut shift=0.;
  for r in &self.regions{
   let start=r.start_quarter*step+shift;if seconds<start{break;}
   if seconds<start+r.seconds.last().unwrap(){let local=seconds-start;let i=r.seconds.partition_point(|v|*v<=local)-1;return r.start_quarter+i as f64+(local-r.seconds[i])/(r.seconds[i+1]-r.seconds[i])*r.segment(i);}
   shift+=r.seconds.last().unwrap()-(r.end()-r.start_quarter)*step;
  }(seconds-shift)/step
 }
 pub fn sample_at(&self,quarter:f64)->f64{self.zero_sample as f64+self.seconds_at(quarter)*self.sample_rate}
 pub fn quarter_at(&self,sample:f64)->f64{self.quarter_at_seconds((sample-self.zero_sample as f64)/self.sample_rate)}
 pub fn tempo_at(&self,quarter:f64)->f64{
  for r in &self.regions{if quarter<r.start_quarter{break;}if quarter<r.end(){let i=(quarter-r.start_quarter).floor()as usize;return 60.*r.segment(i)/(r.seconds[i+1]-r.seconds[i]);}}
  self.bpm
 }
 pub fn position(&self,quarter:f64)->Position{
  let mut signature=Signature::default();let mut anchor=0.;let mut bar=1;
  for event in &self.signatures{if event.quarter>quarter+1e-8{break;}
   let bar_quarters=signature.numerator as f64*4./signature.denominator as f64;
   bar+=((event.quarter-anchor)/bar_quarters-1e-8).ceil()as i64;anchor=event.quarter;signature=*event;
  }
  let elapsed=(quarter-anchor)/(4./signature.denominator as f64);
  let bars=((elapsed+1e-8)/signature.numerator as f64).floor();let within=elapsed-bars*signature.numerator as f64;
  bar+=bars as i64;if bar<=0{bar-=1;}
  Position{signature,signature_quarter:anchor,bar,beat:1+(within+1e-8).floor()as i32,beat_fraction:within-(within+1e-8).floor()}
 }
 pub fn quarter_at_bar(&self,display_bar:i64)->f64{
  let target=if display_bar>0{display_bar}else{display_bar+1};let mut signature=Signature::default();let mut anchor=0.;let mut bar=1;
  for event in &self.signatures{let length=signature.numerator as f64*4./signature.denominator as f64;let next=bar+((event.quarter-anchor)/length-1e-8).ceil()as i64;
   if target<next{return anchor+(target-bar)as f64*length;}bar=next;anchor=event.quarter;signature=*event;
  }anchor+(target-bar)as f64*signature.numerator as f64*4./signature.denominator as f64
 }
 pub fn snap_quarter(&self,quarter:f64,step:f64)->f64{
  let p=self.position(quarter);let mut result=p.signature_quarter+((quarter-p.signature_quarter)/step).round()*step;
  for event in &self.signatures{if(event.quarter-quarter).abs()<(result-quarter).abs(){result=event.quarter;}}result
 }
 pub fn snap_sample(&self,sample:f64,seconds_per_screen:f64,offset_samples:f64,choice:&str)->f64{
  let q=self.quarter_at(sample);let p=self.position(q);
  let center=self.quarter_at(offset_samples+seconds_per_screen*self.sample_rate*0.5);let spq=60./self.tempo_at(center);
  let grid=seconds_for_choice(choice,seconds_per_screen,spq,p.signature.numerator as f64*4./p.signature.denominator as f64);
  if grid<=0.{return sample;}let mut step=grid/spq;
  let span=self.quarter_at(offset_samples+seconds_per_screen*self.sample_rate)-self.quarter_at(offset_samples);
  while span/step>4096.{step*=2.;}
  self.sample_at(self.snap_quarter(q,step)).round().max(0.)
 }
 /// Negative samples must first be rebased in Cardinal. Calculate against a
 /// translated local map, then return the signed result for the atomic proposal.
 pub fn snap_seconds_signed(&self,seconds:f64,span:f64,offset:f64,choice:&str,quantum_seconds:f64)->f64{
  let sample=(seconds*self.sample_rate).round();
  if sample>=0.{return self.snap_sample(sample,span,offset*self.sample_rate,choice)/self.sample_rate;}
  let quantum=(self.sample_rate*quantum_seconds).round().max(1.);
  let shift=(((-sample+quantum*2.)/quantum).ceil()*quantum)as i64;let mut translated=self.clone();translated.zero_sample+=shift;
  (translated.snap_sample(sample+shift as f64,span,offset*self.sample_rate+shift as f64,choice)-shift as f64)/self.sample_rate
 }
 pub fn set_bpm(&mut self,bpm:f64)->bool{
  if !bpm.is_finite()||!(20. ..=400.).contains(&bpm){return false;}
  if self.bpm!=bpm{let ratio=self.bpm/bpm;for r in &mut self.regions{for s in &mut r.seconds{*s*=ratio;}}self.bpm=bpm;}true
 }
 pub fn encode(&self)->Value{json!({"version":1,"bpm":self.bpm,
  "signatures":self.signatures.iter().map(|s|json!({"quarter":s.quarter,"numerator":s.numerator,"denominator":s.denominator})).collect::<Vec<_>>(),
  "rubato_regions":self.regions.iter().map(|r|json!({"start_quarter":r.start_quarter,"seconds":r.seconds,"ripple":r.ripple,"length_quarters":r.length_quarters})).collect::<Vec<_>>()})}
 /// Match the original bounded, permissive project decoder: skip malformed
 /// entries, stable-sort, retain first signature and non-overlapping region.
 pub fn decode(v:&Value)->Self{
  let mut map=Self::default();if let Some(b)=v["bpm"].as_f64().filter(|b|b.is_finite()&&(20. ..=400.).contains(b)){map.bpm=b;}
  for s in v["signatures"].as_array().into_iter().flatten(){if map.signatures.len()>=2048{break;}
   if let (Some(q),Some(n),Some(d))=(s["quarter"].as_f64(),s["numerator"].as_i64(),s["denominator"].as_i64()){
    if !(1..=32).contains(&n)||!(1..=32).contains(&d){continue;}let sig=Signature{quarter:q,numerator:n as i32,denominator:d as i32};if sig.valid(){map.signatures.push(sig);}
   }
  }
  map.signatures.sort_by(|a,b|a.quarter.total_cmp(&b.quarter));map.signatures.dedup_by(|a,b|a.quarter==b.quarter);
  for r in v["rubato_regions"].as_array().into_iter().flatten(){if map.regions.len()>=512{break;}
   let Some(seconds)=r["seconds"].as_array().filter(|a|a.len()<=65537).and_then(|a|a.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())else{continue;};
   let Some(start)=r["start_quarter"].as_f64()else{continue;};
   let Some(ripple)=r.get("ripple").map_or(Some(false),Value::as_bool)else{continue;};
   let Some(length)=r.get("length_quarters").map_or(Some(0.),Value::as_f64)else{continue;};
   let region=Region{start_quarter:start,seconds,ripple,length_quarters:length};if region.valid(){map.regions.push(region);}
  }
  map.regions.sort_by(|a,b|a.start_quarter.total_cmp(&b.start_quarter));let mut end=-1.;map.regions.retain(|r|{if r.start_quarter<end{return false;}end=r.end();true});map
 }
}
pub(crate) const CHOICES:[&str;15]=["adaptive_widest","adaptive_wide","adaptive_medium","adaptive_narrow","adaptive_narrowest","fixed_8_bars","fixed_4_bars","fixed_2_bars","fixed_1_bar","fixed_1_2","fixed_1_4","fixed_1_8","fixed_1_16","fixed_1_32","off"];
pub(crate) fn seconds_for_choice(choice:&str,span:f64,spq:f64,bar:f64)->f64{
 if choice=="off"{return 0.;}
 if choice.starts_with("fixed_"){let beats=match choice{"fixed_8_bars"=>bar*8.,"fixed_4_bars"=>bar*4.,"fixed_2_bars"=>bar*2.,"fixed_1_bar"=>bar,"fixed_1_2"=>2.,"fixed_1_4"=>1.,"fixed_1_8"=>0.5,"fixed_1_16"=>0.25,"fixed_1_32"=>0.125,_=>0.};return beats*spq;}
 let color=if spq<=0.{0.5}else{
  let target=(span/(12.*spq)).max(0.);
  let steps=(0..=10).map(|i|2f64.powi(i-9));
  let beats=steps.into_iter().find(|s|*s>=target).unwrap_or_else(||(0..=8).map(|i|bar.max(0.125)*2f64.powi(i)).find(|s|*s>=target).unwrap_or(bar.max(0.125)*256.));spq*beats
 };
 color*match choice{"adaptive_widest"=>1.,"adaptive_wide"=>0.5,"adaptive_medium"=>0.25,"adaptive_narrow"=>0.125,_=>0.0625}
}
/// Compatibility state for the existing elapsed-seconds document. Free/multi-
/// clock mode remains enabled; musical snapping is independent in Cardinal's
/// loop gesture, and no clip is retimed merely by publishing this map.
#[derive(Clone,Debug,PartialEq)]
pub(crate) struct Timing {pub map:Map,pub revision:u64,pub grid_choice:String,pub multi_clock_mode:bool}
impl Default for Timing {fn default()->Self{Self{map:Map::default(),revision:0,grid_choice:"adaptive_medium".into(),multi_clock_mode:true}}}
impl Timing {
 pub fn snapshot(&self)->Value{let mut value=self.map.encode();value["revision"]=json!(self.revision);value["sample_rate"]=json!(self.map.sample_rate);value["zero_sample"]=json!(self.map.zero_sample);value["grid_choice"]=json!(self.grid_choice);value["multi_clock_mode"]=json!(self.multi_clock_mode);value}
 pub fn from_snapshot(v:&Value)->Self{let mut t=Self::default();t.map=Map::decode(v);
  t.revision=v["revision"].as_u64().unwrap_or(0);t.map.sample_rate=v["sample_rate"].as_f64().filter(|s|s.is_finite()&&*s>0.).unwrap_or(48000.);t.map.zero_sample=v["zero_sample"].as_i64().unwrap_or(0);
  if let Some(c)=v["grid_choice"].as_str().filter(|c|CHOICES.contains(c)){t.grid_choice=c.into();}
  t.multi_clock_mode=v["multi_clock_mode"].as_bool().unwrap_or(true);t
 }
 pub fn for_clock(&self,sample_rate:f64,zero_seconds:f64)->Map{let mut map=self.map.clone();map.sample_rate=sample_rate;map.zero_sample=(zero_seconds*sample_rate).round()as i64;map}
 pub fn quantum(&self)->f64{if self.multi_clock_mode{1.}else{self.map.seconds_per_quarter()*4.}}
 pub fn updated(&self,input:&Value)->Result<Self,String>{let mut next=self.clone();
  if let Some(v)=input.get("map"){if !v.is_object(){return Err("musical map object required".into());}let mut m=Map::decode(v);m.zero_sample=self.map.zero_sample;m.sample_rate=self.map.sample_rate;next.map=m;}
  if let Some(v)=input.get("bpm"){if !next.map.set_bpm(v.as_f64().ok_or("bpm required")?){return Err("bpm outside original 20..400 range".into());}}
  if let Some(v)=input.get("grid_choice"){let c=v.as_str().filter(|c|CHOICES.contains(c)).ok_or("unknown Cardinal grid choice")?;next.grid_choice=c.into();}
  if let Some(v)=input.get("multi_clock_mode"){next.multi_clock_mode=v.as_bool().ok_or("multi_clock_mode must be boolean")?;}
  if next!=*self{next.revision=self.revision.checked_add(1).ok_or("musical time revision exhausted")?;}Ok(next)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn fixture()->Map{Map{bpm:97.,sample_rate:44100.,zero_sample:132300,
  signatures:vec![Signature{quarter:0.,numerator:7,denominator:8},Signature{quarter:3.5,numerator:5,denominator:4},Signature{quarter:12.,numerator:3,denominator:8}],
  regions:vec![Region{start_quarter:1.,seconds:vec![0.,0.3,1.2,1.4,1.75],ripple:false,length_quarters:3.5},Region{start_quarter:7.,seconds:vec![0.,0.8,1.1,2.2],ripple:true,length_quarters:3.}]}}
 #[test]fn original_cpp_oracle_matches_conversion_meter_rubato_and_all_fifteen_grids(){
  // Generated by compiling the unmodified original headers, not this port.
  let oracle:Value=serde_json::from_str(include_str!("MusicalTimeOracle.json")).unwrap();let m=fixture();
  let close=|a:f64,b:f64|assert!((a-b).abs()<1e-8,"{a} != {b}");
  for row in oracle["map"].as_array().unwrap(){let x:Vec<f64>=row.as_array().unwrap().iter().map(|v|v.as_f64().unwrap()).collect();let q=x[0];let p=m.position(q);
   for(a,b)in [m.seconds_at(q),m.quarter_at_seconds(m.seconds_at(q)),m.sample_at(q),m.tempo_at(q),p.bar as f64,p.beat as f64,p.beat_fraction,p.signature_quarter,m.snap_quarter(q,1.)].into_iter().zip(x.into_iter().skip(1)){close(a,b);}
  }
  for row in oracle["grid"].as_array().unwrap(){close(m.snap_sample(row[2].as_f64().unwrap(),row[1].as_f64().unwrap(),7.*m.sample_rate,row[0].as_str().unwrap()),row[3].as_f64().unwrap());}
  for row in oracle["bars"].as_array().unwrap(){close(m.quarter_at_bar(row[0].as_i64().unwrap()),row[1].as_f64().unwrap());}
 }
 #[test]fn fixed_and_ripple_edits_preserve_original_end_behavior(){let mut r=fixture().regions[0].clone();assert!(!r.move_beat(0,0.2));assert!(!r.move_beat(4,3.));assert!(r.move_beat(2,1.3));assert_eq!(*r.seconds.last().unwrap(),1.75);r.ripple=true;assert!(r.move_beat(2,1.5));assert!((r.seconds[3]-1.6).abs()<1e-10);assert!((r.seconds[4]-1.95).abs()<1e-10);}
 #[test]fn source_decoder_orders_filters_and_keeps_first_valid_nonoverlap(){let m=Map::decode(&json!({"bpm":5.,"signatures":[{"quarter":3.,"numerator":5,"denominator":8},{"quarter":0.,"numerator":4,"denominator":3},{"quarter":3.,"numerator":7,"denominator":8}],"rubato_regions":[{"start_quarter":2.,"seconds":[0.,0.5]},{"start_quarter":1.,"seconds":[0.,0.4,0.8]},{"start_quarter":7.,"seconds":[0.,0.]}]}));assert_eq!(m.bpm,120.);assert_eq!(m.signatures.len(),1);assert_eq!(m.signatures[0].numerator,5);assert_eq!(m.regions.len(),1);assert_eq!(m.regions[0].start_quarter,1.);}
 #[test]fn publish_tempo_rescales_rubato_without_changing_quarter_anchors(){let mut m=fixture();let before=m.sample_at(8.)-m.zero_sample as f64;assert!(m.set_bpm(194.));assert!((m.sample_at(8.)-m.zero_sample as f64-before/2.).abs()<1e-8);assert_eq!(m.regions[0].start_quarter,1.);assert!(!m.set_bpm(f64::NAN));}
}

//! Native chains keep DSP state across natural timeline wraps. Output sequence
//! and generation are independent of wrapped project sample positions.
use super::{document::{Content,State},render_clock::{Clock,Sequence}};
pub(super) fn process_clocked(state:&State,track:&str,pcm:&mut[[f32;2]],rate:u32,clock:&Clock,muted:bool,live:Option<(u64,&[super::midi_capture::Voice])>,timeline_playing:bool)->Result<(),String>{
 let frame=|s:f64|(s*rate as f64).round().max(0.)as u64;
 let(start,end)=clock.bounds();let mut cuts=Vec::new();let mut notes=Vec::new();let mut clocks=Vec::new();let mut bends=Vec::new();
 // Prepare track note/clock boundaries once for this whole output block.
 for id in &state.clip_order{let clip=&state.clips[id];if !timeline_playing||clip.track!=track{continue;}
  if let Content::Midi{bpm,numerator,denominator,notes:source,..}=&clip.content{
   let a=frame(clip.start);let b=frame(clip.start+clip.duration());
   if a<end&&b>start{clocks.push((a,b,*bpm,*numerator,*denominator));}
   for at in [a,b]{if at>start&&at<end{cuts.push(at);}}
   for note in source.iter().filter(|n|n.velocity>0.){
    let on=frame(clip.start+note.quarter*60./bpm);let off=frame(clip.start+(note.quarter+note.length)*60./bpm).max(on+1);
    if note.bend!=0. && on<end && off>start {
     bends.push((on,off,note.bend));
    }
    if on<end&&off>start{notes.push((on,off,note.pitch,note.velocity as f32,note.tuning_cents,voice_id(&clip.id,&note.id,on)));}
    for at in [on,off]{if at>start&&at<end{cuts.push(at);}}
   }
  }
 }
 // Add the control clock once per actual rendered span, never per note or
 // across an unrendered seek/loop gap. Work stays bounded by the output block.
 let bend_step=(rate as u64/1000).max(1);
 if !bends.is_empty(){for span in clock.spans(){let mut at=(span.start/bend_step+1)*bend_step;
  while at<span.start+span.frames as u64 {cuts.push(at);at+=bend_step;}
 }}
 cuts.sort_unstable();cuts.dedup();
 // Device capture gates use output offsets, never wrapped project time.
 // Preview voices bypass the track's Off/Auto/In keyboard-monitor gate.
 let mut live_notes=Vec::new();let mut live_cuts=Vec::new();
 if let Some((capture_start,voices))=live{
  let device_end=capture_start.checked_add(pcm.len()as u64).ok_or("live MIDI capture clock overflow")?;
  let input_monitor=state.tracks.get(track).is_some_and(|t|t.io.monitors_input());
  for voice in voices.iter().filter(|v|v.track==track&&(v.preview||input_monitor)){
   let off=voice.off.unwrap_or(u64::MAX);
   if voice.on<device_end&&off>capture_start{
    live_notes.push((voice.on,off,voice.pitch,voice.velocity as f32,voice.tuning_cents,voice_id("live",&voice.pitch.to_string(),voice.on)));
    for at in [voice.on,off]{if at>capture_start&&at<device_end{live_cuts.push((at-capture_start)as usize);}}
   }
  }
 }
 live_cuts.sort_unstable();live_cuts.dedup();
 for span in clock.spans(){
  let mut at=span.start;let finish=at+span.frames as u64;let mut cut=cuts.partition_point(|c|*c<=at);
  let mut live_cut=live_cuts.partition_point(|c|*c<=span.offset);
  while at<finish{
   let offset=span.offset+(at-span.start)as usize;
   let timeline_until=cuts.get(cut).copied().unwrap_or(finish).min(finish);
   let live_until=live_cuts.get(live_cut).copied().unwrap_or(pcm.len()).min(span.offset+span.frames);
   let until=timeline_until.min(at+(live_until-offset)as u64);let mut held=[0f32;128];let mut tuned=Vec::new();
   if !muted{
    for &(on,off,key,v,cents,id)in &notes{if at>=on&&at<off{hold(&mut held,&mut tuned,key,v,cents,id,on)?;}}
    if let Some((capture_start,_))=live{let device_at=capture_start+offset as u64;
     for &(on,off,key,v,cents,id)in &live_notes{if device_at>=on&&device_at<off{hold(&mut held,&mut tuned,key,v,cents,id,on)?;}}
    }
   }
   let musical=clocks.iter().rev().find(|c|at>=c.0&&at<c.1).map_or([120.,0.,4.,4.],|c|[c.2,(at-c.0)as f64/rate as f64*c.2/60.,c.3 as f64,c.4 as f64]);
   let count=(until-at)as usize;
   let sequence=span.sequence.map(|s|Sequence{generation:s.generation,frame:s.frame+at-span.start});
   super::cc_automation::apply(state,track,at as f64/rate as f64)?;
   let bend=if muted {0.} else {channel_bend(&bends,at,bend_step)};
   state.plugins.process_bent_clocked(track,&mut pcm[offset..offset+count],rate,at,musical,&held,sequence,&tuned,bend)?;
   at=until;
   while cuts.get(cut).is_some_and(|c|*c<=at){cut+=1;}
   while live_cuts.get(live_cut).is_some_and(|c|*c<=offset+count){live_cut+=1;}
  }
 }
 Ok(())
}

fn voice_id(clip:&str,note:&str,on:u64)->f64{use std::hash::{Hash,Hasher};let mut h=std::collections::hash_map::DefaultHasher::new();(clip,note,on).hash(&mut h);(h.finish()&((1u64<<52)-1))as f64}
fn hold(held:&mut[f32;128],tuned:&mut Vec<[f64;5]>,key:u8,velocity:f32,cents:f64,id:f64,on:u64)->Result<(),String>{
 let pitch=f64::from(key)+cents/100.;let anchor=pitch.round().clamp(0.,127.)as usize;let offset=(pitch-anchor as f64)*100.;
 if offset.abs()<1e-6{held[anchor]=held[anchor].max(velocity);}else{if tuned.len()>=128{return Err("Native instrument exceeds 128 tuned voices".into());}tuned.push([id,anchor as f64,offset,f64::from(velocity),on as f64]);}Ok(())
}

// Latest onset wins; equal onsets follow the document's stable clip/note order.
fn channel_bend(bends:&[(u64,u64,f64)],at:u64,step:u64)->f64 {
 bends.iter().filter(|b|at>=b.0&&at<b.1).max_by_key(|b|b.0)
  .map_or(0.,|b|super::note_bend::value(b.2,((at/step*step).max(b.0)-b.0)as f64/(b.1-b.0)as f64))
}
#[cfg(test)] mod bend_tests {
 use super::*;
 #[test] fn channel_bend_uses_latest_held_curve_then_resumes_and_recenters(){
  let bends=[(0,1000,2.),(250,750,-2.)];
  assert_eq!(channel_bend(&bends,0,8),0.);
  assert!(channel_bend(&bends,200,8)>0.);
  assert!(channel_bend(&bends,500,8)<0.);
  assert!(channel_bend(&bends,750,8)>0.);
  assert_eq!(channel_bend(&bends,1000,8),0.);
  assert_eq!(channel_bend(&bends,501,8),channel_bend(&bends,503,8));
 }
}

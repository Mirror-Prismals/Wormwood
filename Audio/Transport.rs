//! One application producer and one native callback consumer for real L/S/FF/R PCM.
//! All allocation, project rendering and JSON stay on the producer side.
use super::line_array::{Mixer, Routes};
use std::{
    cell::UnsafeCell,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
};

const CAP: usize = 16384;
struct Queue<T: Copy + Send, const N: usize> {
    slots: Box<[UnsafeCell<T>]>,
    read: AtomicUsize,
    write: AtomicUsize,
}
unsafe impl<T: Copy + Send, const N: usize> Sync for Queue<T, N> {}
impl<T: Copy + Send, const N: usize> Queue<T, N> {
    fn new(value: T) -> Self {
        Self {
            slots: (0..N).map(|_| UnsafeCell::new(value)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }
    fn free(&self) -> usize {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        N - 1 - (w + N - r) % N
    }
    fn push(&self, value: T) -> bool {
        let w = self.write.load(Ordering::Relaxed);
        let next = (w + 1) % N;
        if next == self.read.load(Ordering::Acquire) {
            return false;
        }
        unsafe {
            *self.slots[w].get() = value;
        }
        self.write.store(next, Ordering::Release);
        true
    }
    fn pop(&self) -> Option<T> {
        let r = self.read.load(Ordering::Relaxed);
        if r == self.write.load(Ordering::Acquire) {
            return None;
        }
        let v = unsafe { *self.slots[r].get() };
        self.read.store((r + 1) % N, Ordering::Release);
        Some(v)
    }
}
#[derive(Clone, Copy, Default)]
struct Frame {
    generation: u64,
    next_position: u64,
    sequence: u64,
    buses: [f32; 4],
    monitor: Option<[f32;2]>,
    advance: bool,
}
#[derive(Clone,Copy,Default)]
struct PreviewFrame {generation:u64,position:u64,stereo:[f32;2]}
struct Shared {
    preview:Queue<PreviewFrame,CAP>,
    preview_active:AtomicBool,
    preview_generation:AtomicU64,
    preview_position:AtomicU64,
    preview_length:AtomicU64,
    pcm: Queue<Frame, CAP>,
    routes: Queue<Routes, 8>,
    microphone_routes: Queue<Routes, 8>,
    playing: AtomicBool,
    monitoring:AtomicBool, recording:AtomicBool, generation_start:AtomicU64,
    generation_sequence:AtomicU64, loop_start:AtomicU64, loop_end:AtomicU64, loop_enabled:AtomicBool,
    world: AtomicBool,
    speakers: AtomicBool,
    generation: AtomicU64,
    played_generation: AtomicU64,
    position: AtomicU64,
    underruns: AtomicU64,
    consumed_sequence: AtomicU64,
    gain: AtomicU32,
    peaks: [AtomicU32; 4],
}
/// Deliberately not Clone: the queue has exactly one producer.
/// Prepared on the control thread before a document rebase; committing is
/// infallible and never exposes an audio clock shifted without its document.
pub(crate) struct RebasePlan {position:u64,zero:u64,range:(u64,u64),enabled:bool,generation:u64}
pub(crate) struct Producer {
    shared: Arc<Shared>,
    generation: u64,
    next: u64,
    start: u64,
    rate: u32,
    loop_range: Option<(u64,u64)>,
    loop_enabled: bool,
    sequence: u64,
    generation_sequence: u64,
    timeline_zero:u64,
    preparation_target:usize,
}
pub(crate) struct Output {
    shared: Arc<Shared>,
    mixer: Mixer,
    microphone_mixer: Mixer,
    microphone_sample: f32,
    generation: u64,
    pending: Option<Frame>,
    playing: bool,
    monitoring:bool, recording:bool, loop_range:Option<(u64,u64)>,
    world: bool,
    speakers: bool,
    gain: f32,
    rate: f32,
    starved: bool,
}
pub(crate) fn prepare(rate: u32) -> Result<(Producer, Output), String> {
    let mixer = Mixer::new(rate)?;
    let shared = Arc::new(Shared {
        preview:Queue::new(PreviewFrame::default()),preview_active:AtomicBool::new(false),preview_generation:AtomicU64::new(0),preview_position:AtomicU64::new(0),preview_length:AtomicU64::new(0),
        pcm: Queue::new(Frame::default()),
        routes: Queue::new(Routes::default()),
        microphone_routes: Queue::new(Routes::default()),
        playing: AtomicBool::new(false),
        monitoring:AtomicBool::new(false),recording:AtomicBool::new(false),generation_start:AtomicU64::new(0),
        generation_sequence:AtomicU64::new(0),loop_start:AtomicU64::new(0),loop_end:AtomicU64::new(0),loop_enabled:AtomicBool::new(false),
        world: AtomicBool::new(true),
        speakers: AtomicBool::new(true),
        generation: AtomicU64::new(1),
        played_generation: AtomicU64::new(1),
        position: AtomicU64::new(0),
        underruns: AtomicU64::new(0),
        consumed_sequence: AtomicU64::new(0),
        gain: AtomicU32::new(1f32.to_bits()),
        peaks: std::array::from_fn(|_| AtomicU32::new(0)),
    });
    Ok((
        Producer {
            shared: shared.clone(),
            generation: 1,
            next: 0,
            start: 0,
            rate,
            loop_range: None, loop_enabled: false, sequence: 0, generation_sequence: 0,timeline_zero:0,
            preparation_target:(rate as usize/10).clamp(1024,12288),
        },
        Output {
            shared,
            mixer,
            microphone_mixer: Mixer::new(rate)?,
            microphone_sample: 0.,
            generation: 1,
            pending: None,
            playing: false,
            monitoring:false,recording:false,loop_range:None,
            world: true,
            speakers: true,
            gain: 1.,
            rate: rate as f32,
            starved: false,
        },
    ))
}
impl Producer {
    pub fn set_playing(&mut self, value: bool) {
        self.shared.playing.store(value, Ordering::Release);
    }
    pub(crate) fn preview_reset(&mut self,length:u64) {
        self.shared.preview_active.store(false,Ordering::Release);
        self.shared.preview_generation.fetch_add(1,Ordering::AcqRel);
        self.shared.preview_position.store(0,Ordering::Release);
        self.shared.preview_length.store(length,Ordering::Release);
    }
    pub(crate) fn preview_enable(&mut self,enabled:bool){self.shared.preview_active.store(enabled,Ordering::Release);}
    pub(crate) fn preview_status(&self)->(bool,u64,u64){(self.shared.preview_active.load(Ordering::Acquire),self.shared.preview_position.load(Ordering::Acquire),self.shared.preview_length.load(Ordering::Acquire))}
    pub(crate) fn preview_space(&self)->usize{self.shared.preview.free().min(4096)}
    pub(crate) fn preview_push(&mut self,position:u64,stereo:[f32;2])->bool {
        self.shared.preview.push(PreviewFrame{generation:self.shared.preview_generation.load(Ordering::Acquire),position,stereo})
    }
    pub fn set_monitoring(&mut self,value:bool){self.shared.monitoring.store(value,Ordering::Release);}
    pub fn set_recording(&mut self,value:bool){self.shared.recording.store(value,Ordering::Release);}
    pub fn generation(&self)->u64{self.generation}
    pub fn capacity_frames(&self)->usize{self.shared.pcm.free()}
    pub fn catch_up(&mut self){let consumed=self.shared.consumed_sequence.load(Ordering::Acquire);if self.shared.played_generation.load(Ordering::Acquire)==self.generation&&consumed>self.sequence{self.next=self.position();self.sequence=consumed;}}
    fn publish_capture_clock(&self){
        self.shared.generation_start.store(self.start,Ordering::Relaxed);self.shared.generation_sequence.store(self.generation_sequence,Ordering::Relaxed);
        let(a,b)=self.loop_range.unwrap_or((0,0));self.shared.loop_start.store(a,Ordering::Relaxed);self.shared.loop_end.store(b,Ordering::Relaxed);self.shared.loop_enabled.store(self.loop_enabled,Ordering::Relaxed);
    }
    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }
    pub fn set_world(&mut self, world: bool) {
        self.shared.world.store(world, Ordering::Release);
    }
    pub fn world(&self) -> bool {
        self.shared.world.load(Ordering::Acquire)
    }
    pub fn set_speakers(&mut self, enabled: bool) {
        self.shared.speakers.store(enabled, Ordering::Release);
    }
    pub fn speakers(&self) -> bool {
        self.shared.speakers.load(Ordering::Acquire)
    }
    pub fn set_gain(&mut self, gain: f32) -> Result<(), String> {
        if !gain.is_finite() || !(0.0..=16.).contains(&gain) {
            return Err("master gain must be within 0..16".into());
        }
        self.shared.gain.store(gain.to_bits(), Ordering::Release);
        Ok(())
    }
    pub fn seek(&mut self, frame: u64) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("transport generation exhausted")?;
        // Cardinal AudioSystem.cpp:2565: an explicit discontinuity beyond
        // the loop end resumes at its start; modulo is for natural advance.
        let frame = match (self.loop_enabled,self.loop_range) {
            (true,Some((start,end))) if frame>=end => start,
            _=>frame,
        };
        self.next = frame;
        self.start = frame;
        self.generation_sequence = self.sequence;
        self.publish_capture_clock();
        self.shared
            .generation
            .store(self.generation, Ordering::Release);
        Ok(())
    }
    fn wrap_frame(&self, frame:u64)->u64 {
        if self.loop_enabled {
            if let Some((start,end))=self.loop_range {
                if frame>=end {return start+(frame-start)%(end-start);}
            }
        }
        frame
    }
    /// Changing the range invalidates queued old PCM at the consumed clock.
    /// Natural wraps stay in one generation and never discard pre-wrap frames.
    pub fn set_loop(&mut self, enabled:bool, start:u64, end:u64)->Result<(),String> {
        if start>=end || end>u64::from(self.rate)*86400 {return Err("loop must have a positive range within one day".into());}
        if self.loop_enabled==enabled && self.loop_range==Some((start,end)) {return Ok(());}
        // A disabled range is editing metadata. It must not flush playback,
        // reset DSP history, or change the consumed position/generation.
        if !self.loop_enabled && !enabled {self.loop_range=Some((start,end));return Ok(());}
        self.generation.checked_add(1).ok_or("transport generation exhausted")?;
        let position=self.position();
        self.loop_range=Some((start,end));self.loop_enabled=enabled;
        self.seek(position)
    }
    pub fn prepare_rebase(&self,shift:u64,seek:Option<u64>,range:(u64,u64),enabled:bool)->Result<RebasePlan,String>{
        let end=u64::from(self.rate)*86400;
        let position=seek.or_else(||self.position().checked_add(shift)).ok_or("rebase clock overflow")?;
        let zero=self.timeline_zero.checked_add(shift).ok_or("rebase origin overflow")?;
        if shift==0||position>end||zero>end||range.0>=range.1||range.1>end{return Err("rebase outside one-day transport domain".into());}
        let generation=self.generation.checked_add(1).ok_or("transport generation exhausted")?;
        Ok(RebasePlan{position,zero,range,enabled,generation})
    }
    pub fn commit_rebase(&mut self,plan:RebasePlan){
        self.timeline_zero=plan.zero;self.loop_range=Some(plan.range);self.loop_enabled=plan.enabled;
        self.next=plan.position;self.start=plan.position;self.generation=plan.generation;
        self.generation_sequence=self.sequence;
        self.publish_capture_clock();
        self.shared.generation.store(self.generation,Ordering::Release);
    }
    pub fn initialize_origin(&mut self,frame:u64)->Result<(),String>{
        if frame>u64::from(self.rate)*86400{return Err("timeline origin exceeds one day".into());}
        self.seek(frame)?;self.timeline_zero=frame;Ok(())
    }
    pub fn timeline_zero(&self)->u64{self.timeline_zero}
    pub fn next_frame(&self) -> u64 { self.next }
    /// Production preparation targets two observed native blocks. The legacy
    /// default remains for callers that do not own a device clock.
    pub fn set_preparation_quantum(&mut self,frames:usize){
        self.preparation_target=frames.clamp(64,1024)*2;
    }
    pub fn batch_needed(&self) -> usize {
        // Timeline positions wrap; queue occupancy cannot use next-position.
        let consumed=if self.shared.played_generation.load(Ordering::Acquire)==self.generation {
            self.shared.consumed_sequence.load(Ordering::Acquire)
        }else{self.generation_sequence};
        let queued=self.sequence.saturating_sub(consumed) as usize;
        self.preparation_target
            .saturating_sub(queued).min(4096).min(self.shared.pcm.free())
    }
    pub fn needed(&self)->usize{
        let mut needed=self.batch_needed();
        if self.loop_enabled {
            if let Some((_,end))=self.loop_range {needed=needed.min(end.saturating_sub(self.next) as usize);}
        }
        needed
    }
    pub fn render_clock(&self)->serde_json::Value{
        let mut result=serde_json::json!({"playback_clock":{"generation":self.generation,"sequence":self.sequence}});
        if self.loop_enabled{if let Some((a,b))=self.loop_range{result["loop_range"]=serde_json::json!({"start_frame":a,"end_frame":b});}}result
    }
    pub fn enqueue(&mut self,frames:&[[f32;4]])->Result<(),String>{self.enqueue_inner(frames,false,true)}
    pub fn enqueue_wrapped(&mut self,frames:&[[f32;4]])->Result<(),String>{self.enqueue_inner(frames,true,true)}
    pub fn enqueue_live(&mut self,frames:&[[f32;4]],advance:bool)->Result<(),String>{self.enqueue_inner(frames,true,advance)}
    fn enqueue_inner(&mut self, frames: &[[f32; 4]],wrapped:bool,advance:bool) -> Result<(), String> {self.enqueue_outputs(frames,None,wrapped,advance)}
    pub fn enqueue_outputs(&mut self,frames:&[[f32;4]],monitor:Option<&[[f32;2]]>,wrapped:bool,advance:bool)->Result<(),String>{
        if monitor.is_some_and(|m|m.len()!=frames.len()||m.iter().flatten().any(|v|!v.is_finite())){return Err("invalid monitor PCM submission".into());}
        if frames.len() > self.shared.pcm.free() || frames.iter().flatten().any(|x| !x.is_finite())
        {
            return Err("invalid or overflowing workstation PCM submission".into());
        }
        let end = self
            .next
            .checked_add(frames.len() as u64)
            .ok_or("playback clock overflow")?;
        if advance && !wrapped && self.loop_enabled && self.loop_range.is_some_and(|(_,limit)|end>limit) {
            return Err("PCM submission crosses loop boundary; render the next segment from loop start".into());
        }
        let sequence_end=self.sequence.checked_add(frames.len() as u64).ok_or("transport sequence exhausted")?;
        for (i, buses) in frames.iter().enumerate() {
            if !self.shared.pcm.push(Frame {
                generation: self.generation,
                next_position: if advance{self.wrap_frame(self.next + i as u64 + 1)}else{self.next},
                sequence: self.sequence + i as u64 + 1,
                buses: *buses,
                monitor:monitor.map(|m|m[i]),
                advance,
            }) {
                return Err("workstation PCM ownership violation".into());
            }
        }
        if advance {self.next = self.wrap_frame(end);}
        self.sequence = sequence_end;
        Ok(())
    }
    pub fn microphone_routes(&mut self, routes: Routes) -> Result<bool,String> {
        if routes.count>super::line_array::MAX_CABINETS || routes.voices[..routes.count].iter().any(|r|r.bus>=4 || !r.delay.is_finite() || r.delay<0. || r.bands.iter().chain([&r.left,&r.right]).any(|x|!x.is_finite())) {return Err("invalid microphone cabinets".into());}
        Ok(self.shared.microphone_routes.push(routes))
    }
    pub fn routes(&mut self, routes: Routes) -> Result<bool, String> {
        if routes.count > super::line_array::MAX_CABINETS
            || routes.voices[..routes.count].iter().any(|r| {
                r.bus >= 4
                    || !r.delay.is_finite()
                    || r.delay < 0.
                    || r.bands
                        .iter()
                        .chain([&r.left, &r.right])
                        .any(|x| !x.is_finite())
            })
        {
            return Err("invalid cabinet route snapshot".into());
        }
        Ok(self.shared.routes.push(routes))
    }
    pub fn position(&self) -> u64 {
        if self.shared.played_generation.load(Ordering::Acquire) == self.generation {
            self.shared.position.load(Ordering::Acquire)
        } else {
            self.start
        }
    }
    pub fn peaks(&self) -> [f32; 4] {
        std::array::from_fn(|i| f32::from_bits(self.shared.peaks[i].load(Ordering::Relaxed)))
    }
    pub fn snapshot(&self) -> serde_json::Value {
        let played_generation=self.shared.played_generation.load(Ordering::Acquire);
        let position=if played_generation==self.generation {
            self.shared.position.load(Ordering::Acquire)
        }else{self.start};
        serde_json::json!({"playing":self.playing(),"monitoring":self.shared.monitoring.load(Ordering::Acquire),"mode":if self.world(){"world"}else{"stereo"},"speakers_enabled":self.speakers(),"sample_rate":self.rate,"position_frame":position,"timeline_zero_frame":self.timeline_zero,"loop_enabled":self.loop_enabled,"loop_start_frame":self.loop_range.map(|r|r.0),"loop_end_frame":self.loop_range.map(|r|r.1),"queued_sequence":self.sequence,"consumed_sequence":self.shared.consumed_sequence.load(Ordering::Acquire),"generation_start_frame":self.start,"played_generation":played_generation,"queued_frames":CAP-1-self.shared.pcm.free(),"generation":self.generation,"underruns":self.shared.underruns.load(Ordering::Relaxed),"post_eq_peaks":self.peaks()})
    }
}
impl Output {
    pub fn begin_block(&mut self) {
        for _ in 0..7 {
            if let Some(r) = self.shared.routes.pop() {
                self.mixer.set_routes(r);
            } else {
                break;
            }
        }
        for _ in 0..7 {
            if let Some(routes)=self.shared.microphone_routes.pop(){self.microphone_mixer.set_routes(routes);}else{break;}
        }
        let generation = self.shared.generation.load(Ordering::Acquire);
        let playing = self.shared.playing.load(Ordering::Acquire);
        let world = self.shared.world.load(Ordering::Acquire);
        let speakers = self.shared.speakers.load(Ordering::Acquire);
        if generation != self.generation
            || playing != self.playing
            || world != self.world
            || speakers != self.speakers
        {
            self.mixer.reset();
            self.microphone_mixer.reset();
        }
        if generation != self.generation {
            for _ in 0..CAP {
                let Some(frame) = self.pending.take().or_else(|| self.shared.pcm.pop()) else {
                    break;
                };
                if frame.generation >= generation {
                    self.pending = Some(frame);
                    break;
                }
            }
        }
        self.generation = generation;
        self.playing = playing;
        self.monitoring=self.shared.monitoring.load(Ordering::Acquire);self.recording=self.shared.recording.load(Ordering::Acquire);
        self.loop_range=if self.shared.loop_enabled.load(Ordering::Acquire){Some((self.shared.loop_start.load(Ordering::Relaxed),self.shared.loop_end.load(Ordering::Relaxed)))}else{None};
        self.world = world;
        self.speakers = speakers;
        self.gain = f32::from_bits(self.shared.gain.load(Ordering::Acquire));
        self.starved = false;
        self.mixer.begin_block();
        self.microphone_mixer.begin_block();
    }
    pub fn capture_generation(&self)->u64{self.generation}
    pub fn capture_recording(&self)->bool{self.recording}
    pub fn capture_sequence(&self)->u64{if self.shared.played_generation.load(Ordering::Acquire)==self.generation{self.shared.consumed_sequence.load(Ordering::Acquire)}else{self.shared.generation_sequence.load(Ordering::Acquire)}}
    pub fn capture_position(&self)->(u64,bool){(if self.shared.played_generation.load(Ordering::Acquire)==self.generation{self.shared.position.load(Ordering::Acquire)}else{self.shared.generation_start.load(Ordering::Acquire)},self.playing)}
    pub fn microphone_sample(&self)->f32 {self.microphone_sample}
    pub fn frame(&mut self) -> [f32; 2] {
        self.microphone_sample=0.;
        let mut preview=[0.;2];
        // Bounded SPSC consumption only. Preview never changes the document clock,
        // physical cabinet buses, recording input or microphone receiver.
        let generation=self.shared.preview_generation.load(Ordering::Acquire);
        if self.shared.preview_active.load(Ordering::Acquire) {
            for _ in 0..CAP {let Some(frame)=self.shared.preview.pop()else{break;};if frame.generation!=generation{continue;}
                preview=frame.stereo;self.shared.preview_position.store(frame.position,Ordering::Release);
                if frame.position>=self.shared.preview_length.load(Ordering::Acquire){self.shared.preview_active.store(false,Ordering::Release);}break;
            }
        }
        if !self.playing && !self.monitoring {
            return preview;
        }
        let mut buses = [0.; 4];
        let mut monitor=None;
        let mut found = false;
        // At most one finite queue of stale frames is discarded after a seek.
        // A future-generation frame is retained until its block command arrives.
        for _ in 0..CAP {
            let Some(frame) = self.pending.take().or_else(|| self.shared.pcm.pop()) else {
                break;
            };
            if frame.generation > self.generation {
                self.pending = Some(frame);
                break;
            }
            if frame.generation < self.generation {
                continue;
            }
            buses = frame.buses;
            monitor=frame.monitor;
            found = true;
            if frame.advance&&self.playing || self.shared.played_generation.load(Ordering::Acquire)!=frame.generation {
                self.shared.position.store(frame.next_position,Ordering::Relaxed);
            }
            self.shared.consumed_sequence.store(frame.sequence, Ordering::Relaxed);
            self.shared
                .played_generation
                .store(frame.generation, Ordering::Release);
            break;
        }
        if !found {
            self.starved = true;
            if self.playing&&self.monitoring {
                let mut position=self.capture_position().0.saturating_add(1);
                if let Some((a,b))=self.loop_range{if position>=b{position=a+(position-b)%(b-a);}}
                self.shared.position.store(position,Ordering::Relaxed);
                self.shared.consumed_sequence.store(self.capture_sequence().saturating_add(1),Ordering::Relaxed);
                self.shared.played_generation.store(self.generation,Ordering::Release);
            }
        }
        // Independent receiver of the same physical cabinet buses. Never add
        // this mix to player output or feed it into a world emitter.
        if self.world && self.speakers {
            let mic=self.microphone_mixer.frame(buses);
            let sample=(mic[0]+mic[1])*self.gain;
            self.microphone_sample=if sample.is_finite(){sample}else{0.};
        }
        let out = if self.world {
            if self.speakers {
                self.mixer.frame(buses)
            } else {
                [0.; 2]
            }
        } else {
            let c = 0.5 * (buses[1] + buses[2]);
            monitor.unwrap_or([buses[0] + c, buses[3] + c])
        };
        std::array::from_fn(|i| {
            let y = out[i] * self.gain + preview[i];
            if y.is_finite() { y } else { 0. }
        })
    }
    pub fn end_block(&mut self, frames: usize) {
        if self.starved {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        let peaks = self.mixer.peaks();
        let release = (-(frames as f32) / (self.rate * 0.12)).exp();
        for (i, peak) in peaks.iter().enumerate() {
            let old = f32::from_bits(self.shared.peaks[i].load(Ordering::Relaxed));
            let level = if (self.playing||self.monitoring) && self.world && self.speakers {
                (*peak * self.gain).max(old * release)
            } else {
                0.
            };
            self.shared.peaks[i].store(level.to_bits(), Ordering::Relaxed);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn microphone_cabinets_are_silent_to_player_and_follow_speaker_gate() {
        let (mut p,mut o)=prepare(8000).unwrap();
        let mut routes=Routes::default();routes.count=1;
        routes.voices[0].bus=0;routes.voices[0].delay=0.;routes.voices[0].left=0.5;routes.voices[0].right=0.5;routes.voices[0].bands=[1.;3];
        p.microphone_routes(routes).unwrap();
        let tone:Vec<_>=(0..1024).map(|i|[((i as f32)*0.37).sin(),0.,0.,0.]).collect();
        p.enqueue(&tone).unwrap();p.set_playing(true);
        o.begin_block();let mut peak=0f32;
        for _ in 0..960 {assert_eq!(o.frame(),[0.;2]);peak=peak.max(o.microphone_sample().abs());}
        assert!(peak>0.01,"independent microphone receives physical cabinet buses");
        p.set_speakers(false);o.begin_block();
        for _ in 0..64 {assert_eq!(o.frame(),[0.;2]);assert_eq!(o.microphone_sample(),0.);}
        p.set_world(false);p.set_speakers(true);o.begin_block();
        assert_eq!(o.microphone_sample(),0.);
    }
    #[test]
    fn stereo_uses_four_bus_fold_and_pause_preserves_clock() {
        let (mut p, mut o) = prepare(8000).unwrap();
        p.set_world(false);
        p.enqueue(&[[1., 2., 4., 8.], [2., 0., 0., 3.]]).unwrap();
        p.set_playing(true);
        o.begin_block();
        assert_eq!(o.frame(), [4., 11.]);
        o.end_block(1);
        assert_eq!(p.position(), 1);
        p.set_playing(false);
        o.begin_block();
        assert_eq!(o.frame(), [0.; 2]);
        assert_eq!(p.position(), 1);
        p.set_playing(true);
        o.begin_block();
        assert_eq!(o.frame(), [2., 3.]);
        assert_eq!(p.position(), 2);
    }
    #[test]
    fn seek_discards_old_generation_without_replaying_it() {
        let (mut p, mut o) = prepare(8000).unwrap();
        p.set_world(false);
        p.enqueue(&[[9.; 4]; 100]).unwrap();
        p.seek(500).unwrap();
        let waiting=p.snapshot();
        assert_eq!(waiting["position_frame"],500);
        assert_eq!(waiting["generation_start_frame"],500);
        assert_ne!(waiting["played_generation"],waiting["generation"]);
        p.enqueue(&[[1., 0., 0., 2.]]).unwrap();
        p.set_playing(true);
        o.begin_block();
        assert_eq!(o.frame(), [1., 2.]);
        assert_eq!(p.position(), 501);
        let consumed=p.snapshot();
        assert_eq!(consumed["played_generation"],consumed["generation"]);
        assert_eq!(consumed["generation_start_frame"],500);
        o.end_block(1);
        p.set_playing(false);
        o.begin_block();
        assert_eq!(o.frame(), [0.; 2]);
        o.end_block(1);
        assert_eq!(p.peaks(), [0.; 4]);
    }
    #[test]
    fn speaker_mute_advances_transport_and_does_not_mute_stereo_monitor() {
        let (mut p, mut o) = prepare(8000).unwrap();
        p.enqueue(&[[1., 0., 0., 2.]; 100]).unwrap();
        p.set_playing(true);
        p.set_speakers(false);
        o.begin_block();
        assert_eq!(o.frame(), [0.; 2]);
        o.end_block(1);
        assert_eq!(p.position(), 1);
        assert_eq!(p.peaks(), [0.; 4]);
        p.set_world(false);
        o.begin_block();
        assert_eq!(o.frame(), [1., 2.]);
        o.end_block(1);
        assert_eq!(p.position(), 2);
    }
}

#[cfg(test)]
mod cardinal_loop_tests {
    use super::*;
    #[test] fn queued_audio_wraps_at_exact_sample_without_dropping_pre_wrap_pcm() {
        let (mut p,mut out)=prepare(8000).unwrap();p.set_world(false);p.set_loop(true,2,5).unwrap();
        p.enqueue(&[[0.,0.,0.,0.],[1.,0.,0.,1.],[2.,0.,0.,2.],[3.,0.,0.,3.],[4.,0.,0.,4.]]).unwrap();
        assert_eq!(p.next_frame(),2);let generation=p.snapshot()["generation"].clone();
        p.enqueue(&[[12.,0.,0.,12.],[13.,0.,0.,13.],[14.,0.,0.,14.]]).unwrap();
        p.set_playing(true);out.begin_block();
        for (value,position) in [(0.,1),(1.,2),(2.,3),(3.,4),(4.,2),(12.,3),(13.,4),(14.,2)] {
            assert_eq!(out.frame(),[value,value]);assert_eq!(p.position(),position);
        }
        assert_eq!(p.snapshot()["generation"],generation);assert_eq!(p.snapshot()["consumed_sequence"],8);
    }
    #[test] fn one_sample_loops_have_bounded_queue_and_never_stall_needed() {
        let (mut p,mut out)=prepare(8000).unwrap();p.set_world(false);p.set_loop(true,0,1).unwrap();
        for _ in 0..1024 {assert_eq!(p.needed(),1);p.enqueue(&[[1.,0.,0.,1.]]).unwrap();}
        assert_eq!(p.needed(),0);p.set_playing(true);out.begin_block();
        for _ in 0..400 {assert_eq!(out.frame(),[1.,1.]);assert_eq!(p.position(),0);}
        assert_eq!(p.needed(),1);assert_eq!(p.snapshot()["queued_frames"],624);
    }
    #[test] fn range_edit_flushes_stale_pcm_and_disable_resumes_linear_clock() {
        let (mut p,mut out)=prepare(8000).unwrap();p.set_world(false);p.set_loop(true,0,3).unwrap();
        p.enqueue(&[[9.,0.,0.,9.];3]).unwrap();p.set_loop(true,2,4).unwrap();
        p.enqueue(&[[1.,0.,0.,1.];4]).unwrap();p.set_playing(true);out.begin_block();
        assert_eq!(out.frame(),[1.,1.]);assert_eq!(p.position(),1);
        p.set_loop(false,2,4).unwrap();assert_eq!(p.next_frame(),1);
        p.enqueue(&[[2.,0.,0.,2.];6]).unwrap();out.begin_block();for _ in 0..6{assert_eq!(out.frame(),[2.,2.]);}
        assert_eq!(p.position(),7);
    }
    #[test] fn seek_beyond_end_wraps_and_invalid_loop_does_not_change_state() {
        let (mut p,_)=prepare(8000).unwrap();p.set_loop(true,3,7).unwrap();p.seek(10).unwrap();assert_eq!(p.position(),3);
        let before=p.snapshot();assert!(p.set_loop(true,4,4).is_err());assert_eq!(p.snapshot(),before);
        assert!(p.enqueue(&[[0.;4];5]).is_err());assert_eq!(p.next_frame(),3);
    }
}

#[cfg(test)]mod rebase_tests{
 use super::*;
 #[test]fn rebase_preflight_is_atomic_and_commit_discards_old_pcm_preserving_playing(){
  let(mut p,mut out)=prepare(8000).unwrap();p.set_world(false);p.set_playing(true);
  p.enqueue(&[[9.,0.,0.,9.];4]).unwrap();let before=p.snapshot();
  assert!(p.prepare_rebase(8000*86400+1,None,(1,2),false).is_err());assert_eq!(p.snapshot(),before);
  let plan=p.prepare_rebase(32000,Some(16000),(32000,112000),false).unwrap();assert_eq!(p.snapshot(),before);
  p.commit_rebase(plan);p.enqueue(&[[1.,0.,0.,2.];2]).unwrap();out.begin_block();let frame=out.frame();assert!(frame[0]<9.);assert!(p.playing());assert_eq!(p.timeline_zero(),32000);assert_eq!(p.position(),16001);
 }
}

#[cfg(test)] mod disabled_loop_edit_tests {
 use super::*;
 #[test] fn editing_disabled_range_preserves_queue_generation_and_consumed_position(){
  let(mut p,mut out)=prepare(8000).unwrap();p.set_world(false);
  p.enqueue(&[[1.,0.,0.,1.],[2.,0.,0.,2.],[3.,0.,0.,3.]]).unwrap();p.set_playing(true);out.begin_block();
  assert_eq!(out.frame(),[1.,1.]);let before=p.snapshot();let needed=p.needed();
  for(a,b)in[(2,5),(7,12)]{p.set_loop(false,a,b).unwrap();let after=p.snapshot();
   for key in ["generation","played_generation","position_frame","queued_frames","queued_sequence","consumed_sequence"]{assert_eq!(after[key],before[key],"{key}");}
   assert_eq!(p.next_frame(),3);assert_eq!(p.needed(),needed);
  }
  assert_eq!(out.frame(),[2.,2.]);assert_eq!(out.frame(),[3.,3.]);assert_eq!(p.position(),3);
 }
}

#[cfg(test)] mod preparation_latency_tests {
    use super::*;
    #[test] fn native_quantum_limits_render_ahead_without_changing_samples(){
        for rate in [44100,48000,96000] {
            let(mut producer,mut output)=prepare(rate).unwrap();
            producer.set_world(false);producer.set_preparation_quantum(256);
            assert_eq!(producer.batch_needed(),512);
            let pcm=[[0.75,0.,0.,-1.25];512];
            producer.enqueue_wrapped(&pcm).unwrap();assert_eq!(producer.batch_needed(),0);
            producer.set_playing(true);output.begin_block();
            for _ in 0..128{assert_eq!(output.frame(),[0.75,-1.25]);}
            assert_eq!(producer.batch_needed(),128);
            assert_eq!(producer.snapshot()["underruns"].as_u64().unwrap(),0);
        }
    }
}

#[cfg(test)] mod output_bus_tests {
 use super::*;
 #[test] fn prepared_monitor_is_independent_of_physical_matrices(){
  let (mut p,mut o)=prepare(8000).unwrap();p.set_world(false);
  p.enqueue_outputs(&[[9.,8.,7.,6.]],Some(&[[0.25,-0.5]]),true,true).unwrap();p.set_playing(true);o.begin_block();assert_eq!(o.frame(),[0.25,-0.5]);
  let before=p.snapshot();assert!(p.enqueue_outputs(&[[1.;4]],Some(&[]),true,true).is_err());assert_eq!(p.snapshot(),before);
 }
 #[test] fn world_microphone_receives_only_physical_feed_even_with_loud_monitor(){
  let (mut p,mut o)=prepare(8000).unwrap();let mut routes=Routes::default();routes.count=1;routes.voices[0].bus=0;routes.voices[0].delay=0.;routes.voices[0].left=0.5;routes.voices[0].right=0.5;routes.voices[0].bands=[1.;3];p.microphone_routes(routes).unwrap();
  p.enqueue_outputs(&[[0.;4];16],Some(&[[100.;2];16]),true,true).unwrap();p.set_playing(true);o.begin_block();for _ in 0..16{assert_eq!(o.frame(),[0.;2]);assert_eq!(o.microphone_sample(),0.);}
 }
}

#[cfg(test)] mod browser_preview_tests {
 use super::*;
 #[test] fn preview_pause_resume_replace_and_end_leave_transport_clock_untouched(){
  let(mut p,mut o)=prepare(48000).unwrap();p.preview_reset(3);p.preview_push(1,[0.25,-0.5]);p.preview_push(2,[0.5,-0.25]);p.preview_push(3,[0.75,0.]);p.preview_enable(true);
  o.begin_block();assert_eq!(o.frame(),[0.25,-0.5]);p.preview_enable(false);assert_eq!(o.frame(),[0.;2]);assert_eq!(p.preview_status().1,1);
  p.preview_enable(true);assert_eq!(o.frame(),[0.5,-0.25]);assert_eq!(o.frame(),[0.75,0.]);assert!(!p.preview_status().0);assert_eq!(o.frame(),[0.;2]);
  assert!(!p.playing());assert_eq!(p.position(),0);
  p.preview_reset(1);p.preview_push(1,[1.,1.]);p.preview_reset(1);p.preview_push(1,[-0.5,0.5]);p.preview_enable(true);assert_eq!(o.frame(),[-0.5,0.5]);
 }
 #[test]fn preview_does_not_enter_recording_or_physical_microphone_mix(){let(mut p,mut o)=prepare(48000).unwrap();p.preview_reset(1);p.preview_push(1,[0.3,0.6]);p.preview_enable(true);o.begin_block();assert_eq!(o.frame(),[0.3,0.6]);assert_eq!(o.microphone_sample(),0.);assert_eq!(p.snapshot()["position_frame"],0);}
}

//! One serialized device-callback writer and one application reader.
//! Callback work is bounded and allocation-free. Overflow drops newest input,
//! retaining explicit device-frame gaps rather than fabricating a continuous take.
use std::{cell::UnsafeCell,sync::{Arc,atomic::{AtomicU64,AtomicUsize,Ordering}}};

pub(crate) const MAX_INPUTS: usize = 32;
const CAPACITY: usize = 32768;
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct Frame {
    pub device_frame:u64,
    pub playback_position:u64,
    pub playback_sequence:u64,
    pub playing:bool,
    pub recording:bool,
    pub generation:u64,
    pub physical:[f32;MAX_INPUTS],
    pub physical_count:u8,
    pub virtual_mic:f32,
    pub virtual_mic_available:bool,
    pub player_audio:[f32;2],
}
impl Default for Frame {fn default()->Self{Self{device_frame:0,playback_position:0,playback_sequence:0,playing:false,recording:false,generation:0,physical:[0.;MAX_INPUTS],physical_count:0,virtual_mic:0.,virtual_mic_available:false,player_audio:[0.;2]}}}
struct Shared {
    slots:Box<[UnsafeCell<Frame>]>,read:AtomicUsize,write:AtomicUsize,
    dropped:AtomicU64,physical_count:AtomicUsize,
    next_device_frame:AtomicU64,
}
// Unique non-Clone Writer/Reader own opposite ring indices. Release/Acquire
// transfers slot ownership; neither side reads a slot owned by the other.
unsafe impl Sync for Shared {}
pub(crate) struct Writer {shared:Arc<Shared>,next:u64}
pub(crate) struct Reader {shared:Arc<Shared>,rate:u32}
pub(crate) fn prepare(rate:u32)->Result<(Writer,Reader),String>{
    if !(8000..=192000).contains(&rate){return Err("unsupported capture sample rate".into());}
    let shared=Arc::new(Shared{slots:(0..CAPACITY).map(|_|UnsafeCell::new(Frame::default())).collect(),read:AtomicUsize::new(0),write:AtomicUsize::new(0),dropped:AtomicU64::new(0),physical_count:AtomicUsize::new(0),next_device_frame:AtomicU64::new(0)});
    Ok((Writer{shared:shared.clone(),next:0},Reader{shared,rate}))
}
impl Writer {
    pub fn push(&mut self,mut frame:Frame)->bool{
        frame.device_frame=self.next;self.next=self.next.saturating_add(1);
        self.shared.next_device_frame.store(self.next,Ordering::Release);
        frame.physical_count=frame.physical_count.min(MAX_INPUTS as u8);
        self.shared.physical_count.store(frame.physical_count as usize,Ordering::Relaxed);
        for (channel,value) in frame.physical.iter_mut().enumerate(){if channel>=frame.physical_count as usize||!value.is_finite(){*value=0.;}}
        if !frame.virtual_mic_available||!frame.virtual_mic.is_finite(){frame.virtual_mic=0.;}
        for value in &mut frame.player_audio{if !value.is_finite(){*value=0.;}}
        let write=self.shared.write.load(Ordering::Relaxed);let next=(write+1)%CAPACITY;
        if next==self.shared.read.load(Ordering::Acquire){self.shared.dropped.fetch_add(1,Ordering::Relaxed);return false;}
        unsafe{*self.shared.slots[write].get()=frame;}
        self.shared.write.store(next,Ordering::Release);true
    }
}
impl Reader {
    pub fn sample_rate(&self)->u32{self.rate}
    pub fn next_device_frame(&self)->u64{self.shared.next_device_frame.load(Ordering::Acquire)}
    pub fn dropped_frames(&self)->u64{self.shared.dropped.load(Ordering::Relaxed)}
    pub fn physical_count(&self)->usize{self.shared.physical_count.load(Ordering::Relaxed)}
    pub fn available(&self)->usize{let read=self.shared.read.load(Ordering::Relaxed);let write=self.shared.write.load(Ordering::Acquire);(write+CAPACITY-read)%CAPACITY}
    pub fn drain(&mut self,limit:usize)->Vec<Frame>{
        let mut result=Vec::with_capacity(limit.min(self.available()).min(CAPACITY-1));
        for _ in 0..limit.min(CAPACITY-1){
            let read=self.shared.read.load(Ordering::Relaxed);if read==self.shared.write.load(Ordering::Acquire){break;}
            result.push(unsafe{*self.shared.slots[read].get()});
            self.shared.read.store((read+1)%CAPACITY,Ordering::Release);
        }
        result
    }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn input_sources_clock_and_invalid_samples_remain_distinct(){
  let(mut writer,mut reader)=prepare(48000).unwrap();let mut frame=Frame{physical_count:2,virtual_mic:0.4,virtual_mic_available:true,player_audio:[0.7,-0.6],playback_position:99,playing:true,recording:true,generation:7,..Frame::default()};frame.physical[0]=0.1;frame.physical[1]=0.2;assert!(writer.push(frame));frame.physical[0]=f32::NAN;frame.virtual_mic_available=false;frame.playing=false;assert!(writer.push(frame));
  let values=reader.drain(8);assert_eq!(values.len(),2);assert_eq!(values[0].device_frame,0);assert_eq!(values[1].device_frame,1);assert_eq!(values[0].playback_position,99);assert!(values[0].recording);assert_eq!(values[0].generation,7);assert_eq!(values[0].player_audio,[0.7,-0.6]);assert_eq!(&values[0].physical[..2],&[0.1,0.2]);assert_eq!(values[0].virtual_mic,0.4);assert_eq!(values[1].physical[0],0.);assert_eq!(values[1].virtual_mic,0.);assert!(!values[1].playing);assert_eq!(reader.sample_rate(),48000);assert_eq!(reader.physical_count(),2);assert_eq!(reader.available(),0);
 }
 #[test]fn overflow_retains_existing_audio_and_reports_discontinuity(){
  let(mut writer,mut reader)=prepare(48000).unwrap();for _ in 0..CAPACITY-1{assert!(writer.push(Frame::default()));}assert!(!writer.push(Frame::default()));assert_eq!(reader.dropped_frames(),1);let first=reader.drain(CAPACITY);assert_eq!(first.len(),CAPACITY-1);assert_eq!(first.last().unwrap().device_frame,(CAPACITY-2)as u64);assert!(writer.push(Frame::default()));assert_eq!(reader.drain(1)[0].device_frame,CAPACITY as u64);
 }
 #[test]fn serialized_writer_and_reader_transfer_each_frame_once(){
  let(mut writer,mut reader)=prepare(8000).unwrap();let producer=std::thread::spawn(move||{for i in 0..20000 {assert!(writer.push(Frame{playback_position:i,..Frame::default()}));}});let mut frames=Vec::new();while frames.len()<20000 {frames.extend(reader.drain(1024));std::thread::yield_now();}producer.join().unwrap();assert!(frames.iter().enumerate().all(|(i,f)|f.device_frame==i as u64&&f.playback_position==i as u64));assert_eq!(reader.dropped_frames(),0);
 }
}

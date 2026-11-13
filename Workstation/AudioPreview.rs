//! Browser audition: decode off-thread and feed an independent bounded device queue.
use super::{drum_sample,playback::Source};
use crate::audio::transport::Producer;
use serde_json::{Value,json};
#[derive(Default)]
pub(crate) struct Preview {
    path:Option<String>, source:Option<Source>, pending:Option<std::sync::mpsc::Receiver<Result<Source,String>>>,
    decoding_path:Option<String>, next:u64, requested:bool, rate:u32, error:Option<String>,
}
impl Preview {
    pub(crate) fn toggle(&mut self,path:&str,rate:u32,transport:&mut Producer)->Result<(),String>{
        if !(8000..=192000).contains(&rate)||path.is_empty()||path.len()>4096{return Err("Audio preview needs a valid file and active audio device".into());}
        if self.path.as_deref()==Some(path)&&self.error.is_none(){
            let (active,position,length)=transport.preview_status();
            self.requested=if self.pending.is_some(){!self.requested}else{!active};
            if length>0&&position>=length {self.next=0;transport.preview_reset(length);}
            transport.preview_enable(self.requested&&self.source.is_some());
        }else{
            transport.preview_reset(0);self.path=Some(path.into());self.source=None;self.next=0;self.requested=true;self.rate=rate;self.error=None;
            self.start_decode()?;
        }
        Ok(())
    }
    fn start_decode(&mut self)->Result<(),String>{
        if self.pending.is_some()||self.source.is_some()||!self.requested||self.error.is_some(){return Ok(());}
        let Some(path)=self.path.clone()else{return Ok(());};let file=std::path::PathBuf::from(&path);let(tx,rx)=std::sync::mpsc::channel();
        std::thread::Builder::new().name("browser-audio-decode".into()).spawn(move||{let _=tx.send(drum_sample::read_timeline(&file));}).map_err(|e|e.to_string())?;
        self.decoding_path=Some(path);self.pending=Some(rx);Ok(())
    }
    pub(crate) fn pump(&mut self,transport:&mut Producer){
        if let Some(rx)=&self.pending {match rx.try_recv(){
            Ok(Ok(source))=>{if self.decoding_path!=self.path{self.pending=None;self.decoding_path=None;let _=self.start_decode();return;}let length=((source.samples.len()/source.channels)as f64*self.rate as f64/source.sample_rate as f64).ceil()as u64;self.source=Some(source);self.pending=None;transport.preview_reset(length);transport.preview_enable(self.requested);},
            Ok(Err(e))=>{if self.decoding_path!=self.path{self.pending=None;self.decoding_path=None;let _=self.start_decode();return;}self.error=Some(e);self.pending=None;self.requested=false;transport.preview_enable(false);},
            Err(std::sync::mpsc::TryRecvError::Disconnected)=>{self.error=Some("Preview decoder stopped".into());self.pending=None;self.requested=false;},
            Err(std::sync::mpsc::TryRecvError::Empty)=>{}
        }}
        let Some(source)=&self.source else{return;};let(active,_,length)=transport.preview_status();if !active{return;}
        let frames=source.samples.len()/source.channels;
        for _ in 0..transport.preview_space(){if self.next>=length{break;}
            let at=self.next as f64*source.sample_rate as f64/self.rate as f64;let a=(at.floor()as usize).min(frames-1);let b=(a+1).min(frames-1);let fraction=(at-a as f64)as f32;
            let left=source.stereo(a);let right=source.stereo(b);let sample=std::array::from_fn(|i|left[i]+(right[i]-left[i])*fraction);
            if !transport.preview_push(self.next+1,sample){break;}self.next+=1;
        }
    }
    pub(crate) fn stop(&mut self,transport:&mut Producer){transport.preview_reset(0);self.requested=false;self.path=None;self.source=None;self.next=0;self.error=None;}
    pub(crate) fn status(&self,transport:&Producer)->Value{let(active,position,length)=transport.preview_status();json!({"path":self.path,"playing":active,"loading":self.pending.is_some()&&self.requested,"position_frame":position,"length_frames":length,"error":self.error})}
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn preview_resamples_and_resumes_without_restarting(){
  let(mut p,mut output)=crate::audio::transport::prepare(48000).unwrap();
  let mut preview=Preview{path:Some("test.wav".into()),source:Some(Source{sample_rate:24000,channels:1,samples:std::sync::Arc::from([0.,0.5,1.,0.5])}),rate:48000,requested:true,..Default::default()};
  p.preview_reset(8);p.preview_enable(true);preview.pump(&mut p);output.begin_block();assert_eq!(output.frame(),[0.;2]);assert_eq!(output.frame(),[0.25;2]);
  preview.toggle("test.wav",48000,&mut p).unwrap();assert_eq!(output.frame(),[0.;2]);assert_eq!(p.preview_status().1,2);
  preview.toggle("test.wav",48000,&mut p).unwrap();assert_eq!(output.frame(),[0.5;2]);
  for _ in 0..5{output.frame();}assert!(!p.preview_status().0);preview.toggle("test.wav",48000,&mut p).unwrap();preview.pump(&mut p);assert_eq!(output.frame(),[0.;2]);assert_eq!(p.preview_status().1,1);
 }
}

//! Bounded worker-side input block adapter; never called in an audio callback.
//! Physical channels and VM1 are supplied by the owning audio connection. A
//! missing block means offline playback/export, not a fabricated microphone.
use super::track_io::TrackIo;
use serde_json::Value;

pub(super) struct Block {physical:Vec<Vec<f32>>,virtual_mic:Option<Vec<f32>>}
impl Block {
    pub fn parse(value:&Value,frames:usize)->Result<Option<Self>,String>{
        if value.is_null(){return Ok(None);}
        if !value.is_object(){return Err("live_inputs requires an object".into());}
        let channel=|value:&Value|->Result<Vec<f32>,String>{
            value.as_array().filter(|a|a.len()==frames).ok_or("live input channel must match render frame count")?
                .iter().map(|v|v.as_f64().filter(|v|v.is_finite()&&v.abs()<=64.).map(|v|v as f32).ok_or_else(||"live input samples must be finite within +/-64".into())).collect()
        };
        let physical=value["physical"].as_array().filter(|a|a.len()<=256).ok_or("physical input channels must be a bounded array")?
            .iter().map(channel).collect::<Result<Vec<_>,_>>()?;
        let virtual_mic=value.get("virtual_mic").filter(|v|!v.is_null()).map(channel).transpose()?;
        Ok(Some(Self{physical,virtual_mic}))
    }
    pub fn sample(&self,io:TrackIo,frame:usize)->[f32;2]{
        let get=|channel:usize|self.physical.get(channel).map_or(0.,|s|s[frame]);
        if io.use_virtual_input {let sample=self.virtual_mic.as_ref().map_or(0.,|s|s[frame]);[sample,sample]}
        else if io.stereo_input_pair_12 {[get(0),get(if self.physical.len()>1 {1}else{0})]}
        else {let sample=get(io.physical_input_index);[sample,sample]}
    }
}
#[cfg(test)] mod tests {
    use super::*;use serde_json::json;
    #[test] fn physical_mono_stereo_and_virtual_routing(){
        let b=Block::parse(&json!({"physical":[[1.,2.],[3.,4.]],"virtual_mic":[5.,6.]}),2).unwrap().unwrap();
        let mut io=TrackIo::new("audio");assert_eq!(b.sample(io,0),[1.,1.]);
        io.physical_input_index=1;assert_eq!(b.sample(io,1),[4.,4.]);
        io.physical_input_index=0;io.stereo_input_pair_12=true;assert_eq!(b.sample(io,0),[1.,3.]);
        io.stereo_input_pair_12=false;io.use_virtual_input=true;assert_eq!(b.sample(io,0),[5.,5.]);
        assert!(Block::parse(&json!({"physical":[[1.]]}),2).is_err());
        assert!(Block::parse(&Value::Null,2).unwrap().is_none());
    }
}

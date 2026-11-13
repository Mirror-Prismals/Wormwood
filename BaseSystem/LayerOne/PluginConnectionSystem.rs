//! Desktop plugin control stays on main. Serialized preparation owns DSP and
//! cached automation only; JACK/iOS callbacks never enter this boundary.
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_char, c_void},
};
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn ww_plugins_create() -> *mut c_void;
    fn ww_plugins_destroy(p: *mut c_void);
    fn ww_plugins_command(p: *mut c_void, q: *const c_char) -> *const c_char;
    fn ww_plugins_automate(p:*mut c_void,insert:u64,parameter:u32,value:f64)->bool;
    fn ww_plugins_menu(q: *const c_char) -> i32;
    fn ww_plugins_text_entry(q:*const c_char)->*const c_char;
    fn ww_plugins_process_timeline(
        p: *mut c_void,
        track: *const c_char,
        left: *mut f32,
        right: *mut f32,
        frames: i32,
        rate: f64,
        start: i64,
        bpm: f64,
        quarters: f64,
        numerator: i32,
        denominator: i32,
        held: *const f32,
        discontinuity:i32,
    ) -> bool;
    fn ww_plugins_process_bent_timeline(p:*mut c_void,track:*const c_char,left:*mut f32,right:*mut f32,frames:i32,rate:f64,start:i64,bpm:f64,quarters:f64,numerator:i32,denominator:i32,held:*const f32,discontinuity:i32,tuned:*const f64,count:i32,bend:f64)->bool;
    fn ww_plugins_process_chain_timeline(
        p:*mut c_void,track:*const c_char,left:*mut f32,right:*mut f32,frames:i32,
        rate:f64,start:i64,bpm:f64,quarters:f64,numerator:i32,denominator:i32,
        held:*const f32,chain:*const c_char,discontinuity:i32,
    )->bool;
}
#[cfg(test)]struct TestEffect{track:String,instrument:bool,tail:[f32;2],resets:usize,calls:usize}
#[derive(Default)]
pub(crate) struct Connection {
    #[cfg(test)]test_effect:std::cell::RefCell<Option<TestEffect>>,
    continuity:std::cell::RefCell<std::collections::BTreeMap<String,crate::workstation::render_clock::Continuity>>,
    #[cfg(target_os = "macos")]
    native: Cell<*mut c_void>,
    #[cfg(target_os = "macos")]
    prepared_rate: Cell<u32>,
    #[cfg(target_os = "macos")]
    cached: RefCell<Value>,
    #[cfg(target_os = "macos")]
    cc_cache: RefCell<Option<(Value,Value)>>,
    #[cfg(target_os="macos")]
    capture_clocks:RefCell<std::collections::BTreeMap<String,(u64,u32)>>,
}
impl Connection {
    #[cfg(test)]pub(crate) fn test_install_tail(&self,track:&str,instrument:bool){*self.test_effect.borrow_mut()=Some(TestEffect{track:track.into(),instrument,tail:[0.;2],resets:0,calls:0});}
    #[cfg(test)]pub(crate) fn test_tail_counters(&self)->(usize,usize){let e=self.test_effect.borrow();let e=e.as_ref().unwrap();(e.resets,e.calls)}
    pub(crate) fn status(&self) -> Value {
        #[cfg(target_os = "macos")]
        if !self.cached.borrow().is_null() {
            return self.cached.borrow().clone();
        }
        json!({"available":cfg!(target_os="macos"),"scanned":false,"inserts":[],"catalog":if cfg!(target_os="macos"){json!([{"stock":"LShaping","name":"U Shaping EQ","format":"Stock","instrument":false}])}else{json!([])}})
    }
    /// A separate render host keeps offline DSP and automation away from playback.
    pub(crate) fn clone_track(&self, track:&str, kind:&str, rate:u32, root:&str)->Result<Self,String>{
        self.clone_tracks(&[(track,kind)],rate,root)
    }
    pub(crate) fn clone_tracks(&self, tracks:&[(&str,&str)], rate:u32, root:&str)->Result<Self,String>{
        let copy=Self::default();
        let status=self.status();
        let rows=status["inserts"].as_array().into_iter().flatten().filter(|p|tracks.iter().any(|(id,_)|p["track"]==*id)&&p["chain"].as_str().unwrap_or("playback")=="playback").collect::<Vec<_>>();
        if rows.is_empty(){return Ok(copy);}
        let catalog=if rows.iter().any(|p|p["stock"]!=true){copy.command(&json!({"op":"scan","root":root,"user_root":crate::workstation::audio_library::user_root(std::path::Path::new(root))}))?}else{copy.status()};
        for row in rows{
            let (track,kind)=tracks.iter().copied().find(|(id,_)|row["track"]==*id).unwrap();
            if row["format"]=="VST2"{return Err("VST2 freeze is unavailable; its bridge supports playback only".into());}
            let saved=self.command(&json!({"op":"save","id":row["id"]}))?;
            let mut request=json!({"op":"add","track":track,"track_kind":kind,"sample_rate":rate});
            if row["stock"]==true{request["stock"]=json!("LShaping");}else{
                let found=catalog["catalog"].as_array().into_iter().flatten().find(|c|c["plugin_id"]==row["plugin_id"]&&c["format"]==row["format"]).ok_or("Frozen render plugin is unavailable")?;
                request["index"]=found["index"].clone();
            }
            let added=copy.command(&request)?;
            let id=added["inserts"].as_array().and_then(|a|a.last()).ok_or("Render insert missing")?["id"].clone();
            copy.command(&json!({"op":"restore","id":id,"state":saved["state"]}))?;
            copy.command(&json!({"op":"bypass","id":id,"value":row["bypassed"]}))?;
        }
        Ok(copy)
    }
    pub(crate) fn has_track(&self, track: &str, instrument_only: bool) -> bool {
        #[cfg(test)]if self.test_effect.borrow().as_ref().is_some_and(|e|e.track==track&&(!instrument_only||e.instrument)){return true;}
        #[cfg(target_os = "macos")]
        return self.cached.borrow()["inserts"].as_array().is_some_and(|a| {
            a.iter()
                .any(|p| p["track"] == track && p["chain"].as_str().unwrap_or("playback")=="playback" && (!instrument_only || p["instrument"] == true))
        });
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (track, instrument_only);
            false
        }
    }
    pub(crate) fn has_preamp(&self,track:&str)->bool {
        #[cfg(target_os="macos")]
        return self.cached.borrow()["inserts"].as_array().is_some_and(|rows|rows.iter().any(|p|p["track"]==track&&p["chain"]=="preamp"));
        #[cfg(not(target_os="macos"))] {let _=track;false}
    }
    pub(crate) fn process_preamp(&self,track:&str,pcm:&mut [[f32;2]],rate:u32,capture_start:u64,timeline_start:u64)->Result<(),String>{
        #[cfg(target_os="macos")] {
            if self.native.get().is_null()||pcm.is_empty(){return Ok(());}
            let next_capture=capture_start.checked_add(pcm.len() as u64).filter(|n|*n<=i64::MAX as u64).ok_or("preamp capture clock overflow")?;
            let continuous=self.capture_clocks.borrow().get(track)==Some(&(capture_start,rate));
            let track_key=track.to_string();let track=CString::new(track).map_err(|e|e.to_string())?;let mut left=[0.;2048];let mut right=[0.;2048];
            for (block,chunk) in pcm.chunks_mut(2048).enumerate(){for (i,p) in chunk.iter().enumerate(){left[i]=p[0];right[i]=p[1];}
                let offset=block*2048;let held=[0.;128];
                let ok=unsafe{ww_plugins_process_chain_timeline(self.native.get(),track.as_ptr(),left.as_mut_ptr(),right.as_mut_ptr(),chunk.len() as i32,rate as f64,(timeline_start+offset as u64) as i64,120.,(timeline_start+offset as u64) as f64/rate as f64*2.,4,4,held.as_ptr(),c"preamp".as_ptr(),i32::from(block==0&&!continuous))};
                if !ok{return Err("native capture preamp processing failed".into());}
                for (i,p) in chunk.iter_mut().enumerate(){*p=[left[i],right[i]];}
            }
            self.capture_clocks.borrow_mut().insert(track_key,(next_capture,rate));
        }
        #[cfg(not(target_os="macos"))] let _=(track,pcm,rate,capture_start,timeline_start);
        Ok(())
    }
    pub(crate) fn any(&self) -> bool {
        #[cfg(test)]if self.test_effect.borrow().is_some(){return true;}
        #[cfg(target_os = "macos")]
        return self.cached.borrow()["inserts"]
            .as_array()
            .is_some_and(|a| !a.is_empty());
        #[cfg(not(target_os = "macos"))]
        false
    }
    pub(crate) fn command(&self, q: &Value) -> Result<Value, String> {
        #[cfg(target_os = "macos")]
        {
            let prepared_rate=if q["op"]=="prepare_rate" {
                Some(q["sample_rate"].as_u64().filter(|r|(8000..=384000).contains(r)).ok_or("invalid plugin sample rate")? as u32)
            }else{None};
            if self.native.get().is_null(){if let Some(rate)=prepared_rate{self.prepared_rate.set(rate);return Ok(self.status());}}
            if self.native.get().is_null() {
                if q["op"] == "tick" || q["op"] == "status" || q["op"] == "hide" {
                    return Ok(self.status());
                }
                let p = unsafe { ww_plugins_create() };
                if p.is_null() {
                    return Err("Cannot create native plugin host".into());
                }
                self.native.set(p);
            }
            let mut request_value=q.clone();
            if q["op"]=="add" && q["sample_rate"].is_null() && self.prepared_rate.get()!=0 {
                request_value["sample_rate"]=json!(self.prepared_rate.get());
            }
            let request = CString::new(request_value.to_string()).map_err(|e| e.to_string())?;
            let response = unsafe { ww_plugins_command(self.native.get(), request.as_ptr()) };
            if response.is_null() {
                return Err("Empty native plugin response".into());
            }
            let result: Value =
                serde_json::from_slice(unsafe { CStr::from_ptr(response) }.to_bytes())
                    .map_err(|e| e.to_string())?;
            if let Some(error) = result["error"].as_str() {
                return Err(error.into());
            }
            if let Some(rate)=prepared_rate{self.prepared_rate.set(rate);}
            if result["inserts"].is_array() {
                *self.cached.borrow_mut() = result.clone();
                // Commands are main-thread only. Prewarm controller metadata
                // here so render-side CC evaluation can never query a controller.
                self.automation_catalog(false)?;
            }
            Ok(result)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = q;
            Err("Third-party plugin hosting is excluded on this platform".into())
        }
    }
    /// Metadata cache is invalidated by the real insert graph, and refreshed
    /// explicitly when opening/reselecting an automation target.
    pub(crate) fn automation_catalog(&self,refresh:bool)->Result<Value,String>{
        let status=self.status();
        let graph=Value::Array(status["inserts"].as_array().map_or_else(Vec::new,|rows|rows.iter()
            .filter(|row|row["stock"]!=true&&row["chain"].as_str().unwrap_or("playback")=="playback")
            .map(|row|json!({"id":row["id"],"track":row["track"],"name":row["name"],"instrument":row["instrument"],"format":row["format"]})).collect()));
        if graph.as_array().is_none_or(Vec::is_empty){
            #[cfg(target_os="macos")] {self.cc_cache.borrow_mut().take();}
            return Ok(json!({"devices":[]}));
        }
        #[cfg(target_os="macos")] {
            if !refresh {if let Some((old,catalog))=&*self.cc_cache.borrow(){if old==&graph{return Ok(catalog.clone());}}}
            let catalog=self.command(&json!({"op":"automation_catalog"}))?;
            *self.cc_cache.borrow_mut()=Some((graph,catalog.clone()));Ok(catalog)
        }
        #[cfg(not(target_os="macos"))] {let _=refresh;Ok(json!({"devices":[]}))}
    }
    pub(crate) fn automation_catalog_cached(&self)->Result<Value,String>{
        #[cfg(target_os="macos")] {
            if let Some((_,catalog))=&*self.cc_cache.borrow(){return Ok(catalog.clone());}
            if self.cached.borrow()["inserts"].as_array().is_some_and(|rows|rows.iter().any(|r|r["stock"]!=true&&r["chain"].as_str().unwrap_or("playback")=="playback")) {
                return Err("Plugin automation metadata was not prepared on the main thread".into());
            }
        }
        Ok(json!({"devices":[]}))
    }
    /// Preparation-thread route; native controllers and editor windows remain
    /// on main. Called while the document owner excludes graph/lifecycle edits.
    pub(crate) fn automate(&self,insert:u64,parameter:u32,value:f64)->Result<(),String>{
        #[cfg(target_os="macos")] {
            if self.native.get().is_null(){return Err("Plugin automation has no native host".into());}
            if !unsafe{ww_plugins_automate(self.native.get(),insert,parameter,value)} {
                return Err("Native plugin rejected automation parameter".into());
            }
            Ok(())
        }
        #[cfg(not(target_os="macos"))] {let _=(insert,parameter,value);Err("Native plugin automation is unavailable".into())}
    }
    pub(crate) fn process(&self,track:&str,pcm:&mut[[f32;2]],rate:u32,start:u64,clock:[f64;4],held:&[f32;128])->Result<(),String>{
        self.process_clocked(track,pcm,rate,start,clock,held,None)
    }
    pub(crate) fn process_clocked(
        &self,
        track: &str,
        pcm: &mut [[f32; 2]],
        rate: u32,
        start: u64,
        clock: [f64; 4],
        held: &[f32; 128],
        sequence:Option<crate::workstation::render_clock::Sequence>,
    ) -> Result<(), String> {
        self.process_tuned_clocked(track,pcm,rate,start,clock,held,sequence,&[])
    }
    pub(crate) fn process_tuned_clocked(&self,track:&str,pcm:&mut[[f32;2]],rate:u32,start:u64,clock:[f64;4],held:&[f32;128],sequence:Option<crate::workstation::render_clock::Sequence>,tuned:&[[f64;5]])->Result<(),String>{
        self.process_bent_clocked(track,pcm,rate,start,clock,held,sequence,tuned,0.)
    }
    pub(crate) fn process_bent_clocked(&self,track:&str,pcm:&mut[[f32;2]],rate:u32,start:u64,clock:[f64;4],held:&[f32;128],sequence:Option<crate::workstation::render_clock::Sequence>,tuned:&[[f64;5]],bend:f64)->Result<(),String>{
        #[cfg(test)]if let Some(effect)=self.test_effect.borrow_mut().as_mut().filter(|e|e.track==track){
            let reset=sequence.is_none_or(|s|self.continuity.borrow().get(track).copied().unwrap_or_default().discontinuity(s,rate));
            if reset{effect.tail=[0.;2];effect.resets+=1;}
            effect.calls+=1;
            for sample in pcm.iter_mut(){for ch in 0..2{let input=if effect.instrument{held.iter().sum()}else{sample[ch]};effect.tail[ch]=input+effect.tail[ch]*0.5;sample[ch]=effect.tail[ch];}}
            if let Some(s)=sequence{self.continuity.borrow_mut().entry(track.into()).or_default().commit(s,pcm.len(),rate);}else{self.continuity.borrow_mut().remove(track);}
            return Ok(());
        }
        if sequence.is_none(){self.continuity.borrow_mut().remove(track);}
        #[cfg(target_os = "macos")]
        {
            if self.native.get().is_null() || pcm.is_empty() {
                return Ok(());
            }
            let track_id=track;
            let track = CString::new(track).map_err(|e| e.to_string())?;
            let mut left = [0.; 2048];
            let mut right = [0.; 2048];
            for (block, chunk) in pcm.chunks_mut(2048).enumerate() {
                for (i, p) in chunk.iter().enumerate() {
                    left[i] = p[0];
                    right[i] = p[1];
                }
                let offset = block * 2048;
                let s=sequence.map(|s|crate::workstation::render_clock::Sequence{generation:s.generation,frame:s.frame+offset as u64});
                let reset=s.map_or(-1,|s|i32::from(self.continuity.borrow().get(track_id).copied().unwrap_or_default().discontinuity(s,rate)));
                let ok = unsafe {
                    ww_plugins_process_bent_timeline(
                        self.native.get(),
                        track.as_ptr(),
                        left.as_mut_ptr(),
                        right.as_mut_ptr(),
                        chunk.len() as i32,
                        rate as f64,
                        (start + offset as u64) as i64,
                        clock[0],
                        clock[1] + offset as f64 / rate as f64 * clock[0] / 60.,
                        clock[2] as i32,
                        clock[3] as i32,
                        held.as_ptr(),
                        reset,
                        tuned.as_ptr().cast(),tuned.len()as i32,bend,
                    )
                };
                if !ok {
                    self.continuity.borrow_mut().remove(track_id);
                    return Err("Native plugin processing failed (bends require pitch-wheel support; microtones require native tuning or declared MPE support)".into());
                }
                if let Some(s)=s{self.continuity.borrow_mut().entry(track_id.into()).or_default().commit(s,chunk.len(),rate);}
                for (i, p) in chunk.iter_mut().enumerate() {
                    *p = [left[i], right[i]];
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (track, pcm, rate, start, clock, held,sequence);
        Ok(())
    }
}
#[cfg(target_os = "macos")]
impl Drop for Connection {
    fn drop(&mut self) {
        if !self.native.get().is_null() {
            unsafe {
                ww_plugins_destroy(self.native.get());
            }
        }
    }
}

pub(crate) fn dispatch(
    host: &mut crate::host::Host,
    hook: &str,
    input: &Value,
) -> Result<Value, String> {
    match hook {
        "Status" => host.workstation.with(|s|Ok(s.plugins.status())),
        "Command" => {
            let mut q = input.clone();
            if q["op"] == "scan" {
                q["root"] = json!(host.root().to_string_lossy());
                q["user_root"] = json!(crate::workstation::audio_library::user_root(host.root()));
            }
            if q["op"] == "transfer" {
                let status=host.workstation.with(|s|Ok(s.plugins.status()))?;
                q["track"]=status["inserts"].as_array().and_then(|a|a.iter().find(|p|p["id"]==q["id"])).ok_or("Unknown insert")?["track"].clone();
            }
            if q["op"] == "add" || q["op"] == "transfer" {
                let snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
                let track = snapshot["tracks"].as_array().into_iter().flatten().chain(snapshot["buses"].as_array().into_iter().flatten().filter(|b|(b["kind"]=="mix"||b["kind"]=="channel")))
                    .find(|t|t["id"]==q["track"])
                    .ok_or("Select a track before adding a plugin")?;
                q["track_kind"] = track["kind"].clone();
            }
            host.workstation.with(|document| {
            if document.freeze.busy() && !matches!(q["op"].as_str(),Some("status"|"tick"|"hide"|"automation_catalog")){return Err("Finish or cancel track freeze first".into());}
            let status=document.plugins.status();
            let target=q["track"].as_str().or_else(||status["inserts"].as_array().into_iter().flatten().find(|p|p["id"]==q["id"]).and_then(|p|p["track"].as_str()));
            if target.is_some_and(|id|document.freeze.frozen(id)) && !matches!(q["op"].as_str(),Some("status"|"tick"|"hide"|"automation_catalog")){return Err("Frozen track inserts are locked until flattening".into());}
            let result = document.plugins.command(&q)?;
            if matches!(
                q["op"].as_str(),
                Some("add" | "transfer" | "remove" | "move" | "reorder" | "bypass" | "restore" | "parameter")
            ) {
                document.plugin_changed();
            }
            Ok(result)
            })
        }
        "Menu" => {
            #[cfg(target_os = "macos")]
            {
                if input["text_entry"].is_object() {
                    let request=CString::new(input["text_entry"].to_string()).map_err(|e|e.to_string())?;
                    let reply=unsafe{ww_plugins_text_entry(request.as_ptr())};
                    if reply.is_null(){return Err("Native lane name entry returned no result".into());}
                    return serde_json::from_slice(unsafe{CStr::from_ptr(reply)}.to_bytes()).map_err(|e|e.to_string());
                }
                let rows = CString::new(input["items"].to_string()).map_err(|e| e.to_string())?;
                Ok(json!({"selection":unsafe{ww_plugins_menu(rows.as_ptr())}}))
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = input;
                Err("Plugin menu is desktop only".into())
            }
        }
        _ => Err("Unknown plugin connection hook".into()),
    }
}

#[cfg(all(test,target_os="macos"))]
mod preparation_tests {
    use super::*;
    #[test]
    fn prepare_rate_is_lazy_and_rejects_invalid_rates_without_mutation(){
        let c=Connection::default();
        c.command(&json!({"op":"prepare_rate","sample_rate":44100})).unwrap();
        assert!(c.native.get().is_null());assert_eq!(c.prepared_rate.get(),44100);
        assert!(c.command(&json!({"op":"prepare_rate","sample_rate":0})).is_err());
        assert_eq!(c.prepared_rate.get(),44100);
        assert!(c.native.get().is_null());
    }
    #[test]
    fn worker_metadata_never_creates_a_native_host_and_empty_graph_retires_cache(){
        let c=Connection::default();
        assert_eq!(c.automation_catalog_cached().unwrap(),json!({"devices":[]}));
        *c.cached.borrow_mut()=json!({"inserts":[{"id":1,"track":"a","stock":false}]});
        assert!(c.automation_catalog_cached().is_err());assert!(c.native.get().is_null());
        *c.cc_cache.borrow_mut()=Some((json!([]),json!({"devices":[{"id":1}]})));
        *c.cached.borrow_mut()=json!({"inserts":[]});
        assert_eq!(c.automation_catalog(false).unwrap(),json!({"devices":[]}));
        assert_eq!(c.automation_catalog_cached().unwrap(),json!({"devices":[]}));
    }
}

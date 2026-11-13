//! Native OGC ownership and bounded batch interface. Never used by the JACK callback.
use serde_json::{Value, json};
use std::ffi::{CStr, c_void};
/// Borrowed numeric batches keep the iterative solver out of JSON allocation.
/// The Host still checks the named route, ownership, and records each call.
pub(crate) struct ResponseBatch {
    pub responses: Vec<[f32; 13]>,
    pub contacts: u64,
    pub broad_ms: f64,
    pub narrow_ms: f64,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Stats {
    vertex_face: u64,
    edge_edge: u64,
    truncated: u64,
    overflow: u64,
    broad_ms: f64,
    narrow_ms: f64,
}
#[cfg(any(target_os = "macos",target_os="ios"))]
unsafe extern "C" {
    fn ww_ogc_create(radius: f32, stiffness: f32) -> *mut c_void;
    fn ww_ogc_destroy(p: *mut c_void);
    fn ww_ogc_parallel_queries(p: *mut c_void, enabled: i32);
    fn ww_ogc_error(p: *mut c_void) -> *const std::ffi::c_char;
    fn ww_ogc_surface(
        p: *mut c_void,
        positions: *const f32,
        count: usize,
        faces: *const u32,
        face_count: usize,
        motion: i32,
        self_collision: i32,
        friction: f32,
    ) -> u64;
    fn ww_ogc_sync(p:*mut c_void,handle:u64,positions:*const f32,previous:*const f32,count:usize)->i32;
    fn ww_ogc_filter(p:*mut c_void,handle:u64,elements:*const u32,count:usize,pairs:*const u32,pair_count:usize)->i32;
    fn ww_ogc_active_vertices(p:*mut c_void,handle:u64,active:*const u8,count:usize)->i32;
    fn ww_ogc_movable_vertices(p:*mut c_void,handle:u64,movable:*const u8,count:usize)->i32;
    fn ww_ogc_build(p: *mut c_void) -> i32;
    fn ww_ogc_project(
        p: *mut c_void,
        handle: u64,
        positions: *mut f32,
        previous: *const f32,
        inverse_mass: *const f32,
        count: usize,
        dt: f32,
    ) -> i32;
    fn ww_ogc_response(p:*mut c_void,handle:u64,output:*mut f32,count:usize)->i32;
    fn ww_ogc_forces(p: *mut c_void, handle: u64, forces: *mut f32, count: usize) -> i32;
    fn ww_ogc_stats(p: *mut c_void) -> Stats;
}
#[derive(Default)]
pub(crate) struct State {
    pointer: *mut c_void,
    owner: Option<String>,
    surfaces: Vec<(u64, usize, usize)>,
    count: usize,
}
impl Drop for State {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos",target_os="ios"))]
        if !self.pointer.is_null() {
            unsafe {
                ww_ogc_destroy(self.pointer);
            }
        }
    }
}
impl State {
    pub(crate) fn respond(&mut self, positions: &[[f32; 3]], previous: &[[f32; 3]]) -> Result<ResponseBatch, String> {
        if positions.len() != self.count || previous.len() != self.count || self.count == 0
            || positions.iter().chain(previous).flatten().any(|v| !v.is_finite()) {
            return Err("invalid OGC response batch".into());
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        { Err("native OGC response unavailable on this platform".into()) }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            if self.pointer.is_null() { return Err("OGC is not configured".into()); }
            for &(handle, start, count) in &self.surfaces {
                if ww_ogc_sync(self.pointer, handle, positions[start..].as_ptr().cast(), previous[start..].as_ptr().cast(), count) == 0 {
                    return Err(self.error());
                }
            }
            if ww_ogc_build(self.pointer) == 0 { return Err(self.error()); }
            let stats = ww_ogc_stats(self.pointer);
            if stats.overflow > 0 { return Err(format!("OGC contact capacity exceeded: {} dropped pairs", stats.overflow)); }
            let mut responses = vec![[0f32; 13]; self.count];
            for &(handle, start, count) in &self.surfaces {
                if ww_ogc_response(self.pointer, handle, responses[start..].as_mut_ptr().cast(), count) == 0 { return Err(self.error()); }
            }
            if responses.iter().flatten().any(|x| !x.is_finite()) { return Err("non-finite OGC response".into()); }
            Ok(ResponseBatch { responses, contacts: stats.vertex_face + stats.edge_edge, broad_ms: stats.broad_ms, narrow_ms: stats.narrow_ms })
        }
    }
    pub(crate) fn respond_json(&mut self, input: &Value) -> Result<Value, String> {
        let positions: Vec<[f32; 3]> = serde_json::from_value(input["positions"].clone()).map_err(|e|e.to_string())?;
        let previous: Vec<[f32; 3]> = serde_json::from_value(input["previous"].clone()).map_err(|e|e.to_string())?;
        let batch = self.respond(&positions, &previous)?;
        Ok(json!({"responses":batch.responses,"contacts":batch.contacts,"broad_ms":batch.broad_ms,"narrow_ms":batch.narrow_ms}))
    }
    pub(crate) fn ownership(&self, caller: &str) -> Value {
        json!({"owner":self.owner,"configured":self.owner.is_some(),"owned_by_caller":self.owner.as_deref()==Some(caller)})
    }
    pub(crate) fn assert_owner(&self, caller: &str) -> Result<(), String> {
        if self.owner.as_deref().is_some_and(|owner| owner != caller) {
            return Err(format!("OGC context owned by {}; {caller} cannot replace, project or release it", self.owner.as_deref().unwrap()));
        }
        Ok(())
    }
    pub(crate) fn configure_for(&mut self, caller: &str, scene: &Value) -> Result<Value,String> {
        self.assert_owner(caller)?;
        let result = self.configure(scene)?;
        self.owner = Some(caller.into());
        Ok(result)
    }
    pub(crate) fn release_for(&mut self, caller: &str) -> Result<Value,String> {
        self.assert_owner(caller)?;
        *self=Self::default();
        Ok(Value::Null)
    }
    #[cfg(any(target_os = "macos",target_os="ios"))]
    fn error(&self) -> String {
        unsafe {
            CStr::from_ptr(ww_ogc_error(self.pointer))
                .to_string_lossy()
                .into_owned()
        }
    }
    fn configure(&mut self, scene: &Value) -> Result<Value, String> {
        #[cfg(not(any(target_os = "macos",target_os="ios")))]
        {
            let _ = scene;
            return Err("OGC workshop native client is currently enabled on macOS".into());
        }
        #[cfg(any(target_os = "macos",target_os="ios"))]
        unsafe {
            let positions: Vec<[f32; 3]> =
                serde_json::from_value(scene["positions"].clone()).map_err(|e| e.to_string())?;
            if positions.len() > 8192 {
                return Err("OGC node budget exceeded".into());
            }
            let mut next = Self::default();
            next.pointer = ww_ogc_create(
                scene["contact_radius"].as_f64().unwrap_or(0.008) as f32,
                scene["contact_stiffness"].as_f64().unwrap_or(5000.) as f32,
            );
            if next.pointer.is_null() {
                return Err("OGC allocation failed".into());
            }
            ww_ogc_parallel_queries(next.pointer, i32::from(scene["parallel_queries"] != false));
            next.count = positions.len();
            for surface in scene["surfaces"].as_array().ok_or("surfaces required")? {
                let start = surface["start"].as_u64().ok_or("surface start required")? as usize;
                let count = surface["count"].as_u64().ok_or("surface count required")? as usize;
                if count == 0
                    || start
                        .checked_add(count)
                        .is_none_or(|end| end > positions.len())
                {
                    return Err("surface range invalid".into());
                }
                let faces: Vec<[u32; 3]> = serde_json::from_value(surface["triangles"].clone())
                    .map_err(|e| e.to_string())?;
                if faces.is_empty()
                    || faces.len() > 8192
                    || faces.iter().flatten().any(|i| *i as usize >= count)
                {
                    return Err("surface topology invalid".into());
                }
                let h = ww_ogc_surface(
                    next.pointer,
                    positions[start..].as_ptr().cast(),
                    count,
                    faces.as_ptr().cast(),
                    faces.len(),
                    surface["motion"].as_i64().unwrap_or(0) as i32,
                    i32::from(surface["self_collision"] != false),
                    surface["friction"].as_f64().unwrap_or(0.3) as f32,
                );
                if h == 0 {
                    return Err(next.error());
                }
                if !surface["active_vertices"].is_null() {
                    let active:Vec<bool>=serde_json::from_value(surface["active_vertices"].clone()).map_err(|e|e.to_string())?;
                    if active.len()!=count || faces.iter().flatten().any(|i|!active[*i as usize]) {
                        return Err("invalid OGC active vertex mask".into());
                    }
                    let active:Vec<u8>=active.into_iter().map(u8::from).collect();
                    if ww_ogc_active_vertices(next.pointer,h,active.as_ptr(),active.len())==0{return Err("OGC active vertex mask rejected".into());}
                }
                if !surface["movable_vertices"].is_null() {
                    let movable: Vec<bool> = serde_json::from_value(surface["movable_vertices"].clone()).map_err(|e|e.to_string())?;
                    if movable.len() != count { return Err("invalid OGC movable vertex mask".into()); }
                    let movable: Vec<u8> = movable.into_iter().map(u8::from).collect();
                    if ww_ogc_movable_vertices(next.pointer,h,movable.as_ptr(),count)==0 { return Err(next.error()); }
                }
                if !surface["element_ids"].is_null() {
                    let labels:Vec<u32>=serde_json::from_value(surface["element_ids"].clone()).map_err(|e|e.to_string())?;
                    let pairs:Vec<[u32;2]>=serde_json::from_value(surface["excluded_pairs"].clone()).map_err(|e|e.to_string())?;
                    if labels.len()!=count || pairs.len()>8192 {return Err("invalid cohesive contact filter".into());}
                    if ww_ogc_filter(next.pointer,h,labels.as_ptr(),labels.len(),pairs.as_ptr().cast(),pairs.len())==0{return Err(next.error());}
                }
                next.surfaces.push((h, start, count));
            }
            if ww_ogc_build(next.pointer) == 0 {
                return Err(next.error());
            }
            *self = next;
            Ok(Value::Null)
        }
    }
    pub(crate) fn synchronize(&mut self,input:&Value)->Result<Value,String>{
        #[cfg(not(any(target_os="macos",target_os="ios")))] {let _=input;Err("native OGC synchronization unavailable on this platform".into())}
        #[cfg(any(target_os="macos",target_os="ios"))] unsafe {
            let start=input["surface_start"].as_u64().ok_or("surface start required")? as usize;
            let &(handle,_,count)=self.surfaces.iter().find(|(_,s,_)|*s==start).ok_or("unknown OGC synchronization surface")?;
            let p:Vec<[f32;3]>=serde_json::from_value(input["positions"].clone()).map_err(|e|e.to_string())?;
            let old:Vec<[f32;3]>=serde_json::from_value(input["previous"].clone()).map_err(|e|e.to_string())?;
            if p.len()!=count||old.len()!=count||p.iter().chain(&old).flatten().any(|x|!x.is_finite()){return Err("invalid OGC synchronized positions".into());}
            if ww_ogc_sync(self.pointer,handle,p.as_ptr().cast(),old.as_ptr().cast(),count)==0{return Err(self.error());}
            Ok(Value::Null)
        }
    }
    pub(crate) fn project(&mut self, input: &Value) -> Result<Value, String> {
        #[cfg(not(any(target_os = "macos",target_os="ios")))]
        {
            let _ = input;
            Err("OGC workshop requires macOS".into())
        }
        #[cfg(any(target_os = "macos",target_os="ios"))]
        unsafe {
            if self.pointer.is_null() {
                return Err("OGC is not configured".into());
            }
            let mut positions: Vec<[f32; 3]> =
                serde_json::from_value(input["positions"].clone()).map_err(|e| e.to_string())?;
            let previous: Vec<[f32; 3]> =
                serde_json::from_value(input["previous"].clone()).map_err(|e| e.to_string())?;
            let weights: Vec<f32> =
                serde_json::from_value(input["inverse_mass"].clone()).map_err(|e| e.to_string())?;
            if positions.len() != self.count
                || previous.len() != self.count
                || weights.len() != self.count
            {
                return Err("OGC batch size mismatch".into());
            }
            if !input["excluded_pairs"].is_null() {
                let pairs:Vec<[u32;2]>=serde_json::from_value(input["excluded_pairs"].clone()).map_err(|e|e.to_string())?;
                if pairs.len()>8192{return Err("cohesive exclusion budget exceeded".into());}
                if ww_ogc_filter(self.pointer,self.surfaces[0].0,std::ptr::null(),0,pairs.as_ptr().cast(),pairs.len())==0{return Err(self.error());}
            }
            if ww_ogc_build(self.pointer) == 0 {
                return Err(self.error());
            }
            let mut stats = ww_ogc_stats(self.pointer);
            if stats.overflow > 0 {
                return Err(format!(
                    "OGC contact capacity exceeded: {} dropped pairs",
                    stats.overflow
                ));
            }
            let mut rigid_responses=Vec::<[f32;13]>::new();
            let mut all_responses=Vec::<[f32;13]>::new();
            if input["all_responses"] == true {
                all_responses.resize(self.count,[0.;13]);
                for &(handle,start,count) in &self.surfaces {
                    if ww_ogc_response(self.pointer,handle,all_responses[start..].as_mut_ptr().cast(),count)==0 {return Err(self.error());}
                }
            }
            if let Some(at)=input["rigid_response_start"].as_u64(){
                let &(handle,_,count)=self.surfaces.iter().find(|(_,start,_)|*start==at as usize).ok_or("rigid response surface does not exist")?;
                rigid_responses.resize(count,[0.;13]);
                if ww_ogc_response(self.pointer,handle,rigid_responses.as_mut_ptr().cast(),count)==0{return Err(self.error());}
            }
            let mut forces = vec![[0f32; 3]; self.count];
            for &(h, start, count) in &self.surfaces {
                if ww_ogc_forces(self.pointer, h, forces[start..].as_mut_ptr().cast(), count) == 0 {
                    return Err(self.error());
                }
                if ww_ogc_project(
                    self.pointer,
                    h,
                    positions[start..].as_mut_ptr().cast(),
                    previous[start..].as_ptr().cast(),
                    weights[start..].as_ptr(),
                    count,
                    input["dt"].as_f64().unwrap() as f32,
                ) == 0
                {
                    return Err(self.error());
                }
            }
            stats.truncated = ww_ogc_stats(self.pointer).truncated;
            Ok(
                json!({"positions":positions,"forces":forces,"rigid_responses":rigid_responses,"all_responses":all_responses,"vertex_face_contacts":stats.vertex_face,"edge_edge_contacts":stats.edge_edge,"truncated_displacements":stats.truncated,"contact_overflows":stats.overflow,"broad_ms":stats.broad_ms,"narrow_ms":stats.narrow_ms}),
            )
        }
    }
}
pub(crate) fn valid_scene(v: &Value) -> bool {
    v["positions"]
        .as_array()
        .is_some_and(|p| !p.is_empty() && p.len() <= 8192 && p.iter().all(point))
        && v["surfaces"].is_array()
        && v["contact_radius"]
            .as_f64()
            .is_some_and(|r| r.is_finite() && r > 0. && r <= 1.)
}
fn point(v: &Value) -> bool {
    v.as_array().is_some_and(|p| {
        p.len() == 3
            && p.iter().all(|x| {
                x.as_f64()
                    .is_some_and(|x| x.is_finite() && (x as f32).is_finite())
            })
    })
}
pub(crate) fn valid_request(v: &Value) -> bool {
    (v["all_responses"].is_null() || v["all_responses"].is_boolean())
        && v["positions"]
        .as_array()
        .is_some_and(|p| p.len() <= 8192 && p.iter().all(point))
        && v["previous"]
            .as_array()
            .is_some_and(|p| p.len() <= 8192 && p.iter().all(point))
        && v["inverse_mass"].as_array().is_some_and(|p| {
            p.len() <= 8192
                && p.iter()
                    .all(|x| x.as_f64().is_some_and(|x| x.is_finite() && x >= 0.))
        })
        && v["dt"].as_f64().is_some_and(|dt| dt > 0. && dt <= 1. / 30.)
}

pub(crate) fn valid_sync(v:&Value)->bool{
    v["surface_start"].as_u64().is_some_and(|s|s<8192)
    &&v["positions"].as_array().is_some_and(|p|!p.is_empty()&&p.len()<=8192&&p.iter().all(point)&&v["previous"].as_array().is_some_and(|old|old.len()==p.len()&&old.iter().all(point)))
}
pub(crate) fn valid_response_request(v: &Value) -> bool {
    v["positions"].as_array().is_some_and(|p| !p.is_empty() && p.len()<=8192 && p.iter().all(point)
        && v["previous"].as_array().is_some_and(|old|old.len()==p.len() && old.iter().all(point)))
}

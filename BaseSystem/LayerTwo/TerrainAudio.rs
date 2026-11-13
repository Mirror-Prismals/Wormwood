//! RayTracedAudioSystem adapter for accepted continuous terrain snapshots.
use super::ray_traced_audio_system::{State, listener};
use crate::audio::{acoustic::Source, geometry::Geometry, vector};
use serde_json::Value;
// Per receiver, on the acoustic control/worker side, never the audio callback.
#[derive(Default)]
pub(crate) struct WindCover {
    height: f64,
    last_time: Option<f64>,
    next_probe: f64,
    target: f64,
    opening: crate::audio::V,
    openness: f64,
    offset: crate::audio::V,
}
impl WindCover {
    fn position(&mut self, geometry: &impl Geometry, position: crate::audio::V, up: crate::audio::V, time: f64) -> crate::audio::V {
        use crate::audio::{add, mul, unit, cross, length};
        let up = unit(up);
        let dt = self.last_time.map_or(0., |t| (time-t).clamp(0., 0.25));
        if self.last_time.is_none() || time >= self.next_probe {
            // Look down from outside rather than stopping at the first ceiling:
            // this clears thick roofs, multiple storeys and underground cover.
            let top = add(position, mul(up, 128.));
            self.target = geometry.trace(top, mul(up, -1.), 127.95)
                .map_or(2., |hit| (128.-hit.distance+1.).max(2.));
            // Direct escape probes sample solid angle, not reflected rays that
            // can eventually escape a remote corridor. Use the receiver's local up.
            let right = unit(cross(up, if up[0].abs()<0.8 {[1.,0.,0.]}else{[0.,0.,1.]}));
            let forward = cross(right, up);
            let mut sum=[0.;3]; let mut escaped=0.;
            if self.target>2. {
                for i in 0..96 {
                    let y=(i as f64+0.5)/96.;
                    let a=i as f64*2.399963229728653;
                    let r=(1.-y*y).sqrt();
                    let direction=add(mul(up,y),add(mul(right,r*a.cos()),mul(forward,r*a.sin())));
                    if geometry.trace(position,direction,64.).is_none()
                        && geometry.trace(add(position,mul(direction,64.)),up,128.).is_none() {
                        escaped+=1.; sum=add(sum,direction);
                    }
                }
            }
            self.openness=(escaped/12_f64).clamp(0.,1.);
            self.opening=if length(sum)>0.001 {unit(sum)}else{up};
            self.next_probe = time + 0.2;
        }
        // One continuous pose smoother handles both sides of an opening;
        // a roof-edge hold would lift the source again just after leaving a door.
        self.height = self.target.max(2.);
        self.last_time = Some(time);
        let outside=mul(up,self.height);
        let doorway=mul(self.opening,2.);
        let target=add(mul(outside,1.-self.openness),mul(doorway,self.openness));
        if dt==0. {self.offset=target;}else{
            self.offset=add(self.offset,mul(crate::audio::sub(target,self.offset),1.-(-dt/0.35).exp()));
        }
        add(position,self.offset)
    }
}
pub(crate) fn advance(state: &mut State, input: &Value) -> Result<Value, String> {
    advance_parts(state, &input["geometry"], &input["listener"], &input["sources"], input["time"].as_f64().ok_or("audio time required")?)
}
pub(crate) fn advance_parts(state: &mut State, geometry_input: &Value, listener_input: &Value, sources_input: &Value, time: f64) -> Result<Value, String> {
    let revision = geometry_input["revision"]
        .as_u64()
        .ok_or("geometry revision required")?;
    // Freeze accepted geometry through one multi-frame ray batch. Moving
    // fragments and wind must not invalidate the batch before it can publish.
    if state.terrain_geometry.is_none()
        || (!state.engine.batch_active()
            && state
                .terrain_geometry
                .as_ref()
                .is_none_or(|g| g.revision() != revision))
    {
        state.terrain_geometry = Some(std::sync::Arc::new(crate::terrain::material_geometry::Surface::parse(
            geometry_input,
        )?));
    }
    let geometry = state
        .terrain_geometry
        .as_deref()
        .ok_or("terrain geometry missing")?;
    let rows = sources_input
        .as_array()
        .filter(|rows| rows.len() <= 17)
        .ok_or("bounded acoustic sources required")?;
    let receiver = listener(listener_input)?;
    let mut sources = Vec::with_capacity(rows.len());
    for row in rows {
        sources.push(Source {
            id: row["id"].as_u64().ok_or("source id required")?,
            channel: row["channel"]
                .as_u64()
                .filter(|n| *n < 17)
                .ok_or("source channel required")? as usize,
            position: if row["outdoor_wind"] == true {
                state.wind_cover.position(geometry, receiver.position, receiver.up, time)
            } else { vector(&row["position"])? },
        });
    }
    // Keep the full reflection batch, spreading its terrain queries across frames.
    state.engine.advance_budgeted(
        geometry,
        listener(listener_input)?,
        &sources,
        time,
        1.,
        2,
    )?;
    Ok(state.snapshot())
}

// A single in-flight job. The game never waits on an acoustic query and the
// worker never owns a Host, graphics object, native callback or mutable world.
use std::sync::{Arc,mpsc};
struct Request { geometry:Arc<Value>,listener:Value,sources:Value,time:f64,cabinets:Vec<[f64;3]>,retired:Option<Arc<Value>> }
struct Completed {
    snapshot:Value,
    geometry:Arc<crate::terrain::material_geometry::Surface>,
    responses:std::collections::BTreeMap<u64,crate::audio::acoustic::Response>,
    segments:Vec<crate::audio::acoustic::Segment>,
    version:u64,escape:Option<f64>,rig:Vec<Option<crate::audio::geometry::Hit>>,
}
pub(crate) struct Worker {
    requests:Option<mpsc::SyncSender<Request>>,
    completed:mpsc::Receiver<Result<Completed,String>>,
    thread:Option<std::thread::JoinHandle<()>>,
    busy:bool,
    geometry:Option<Arc<Value>>,
    snapshot:Value,
}
impl Worker {
    fn start()->Result<Self,String> {
        let(tx,rx)=mpsc::sync_channel::<Request>(1);
        let(done_tx,done_rx)=mpsc::sync_channel(1);
        let thread=std::thread::Builder::new().name("mirror-terrain-acoustics".into()).spawn(move|| {
            let mut state=State::default();
            let mut rig_cache:Option<(u64,[u64;3],Vec<[f64;3]>,Vec<Option<crate::audio::geometry::Hit>>)>=None;
            while let Ok(request)=rx.recv() {
                let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::Audio);
                let result=advance_parts(&mut state,&request.geometry,&request.listener,&request.sources,request.time)
                    .and_then(|snapshot|{
                        let geometry=state.terrain_geometry.as_ref().ok_or("acoustic geometry missing")?;
                        let position=vector(&request.listener["position"])?;
                        let key=(geometry.revision(),position.map(f64::to_bits));
                        if rig_cache.as_ref().is_none_or(|(revision,pose,cabinets,_)|(*revision,*pose)!=key||*cabinets!=request.cabinets){
                            let hits=request.cabinets.iter().map(|&cabinet|{
                                let delta=crate::audio::sub(position,cabinet);let distance=crate::audio::length(delta);
                                if distance>0.04 && distance<180.{geometry.trace(cabinet,crate::audio::unit(delta),distance-0.02)}else{None}
                            }).collect();
                            rig_cache=Some((key.0,key.1,request.cabinets,hits));
                        }
                        Ok(Completed{snapshot,rig:rig_cache.as_ref().unwrap().3.clone(),geometry:state.terrain_geometry.as_ref().ok_or("acoustic geometry missing")?.clone(),
                        responses:state.engine.responses.clone(),segments:state.engine.debug.clone(),version:state.engine.version,escape:state.engine.escape})});
                if done_tx.send(result).is_err(){break;}
            }
        }).map_err(|e|format!("cannot start terrain acoustics: {e}"))?;
        Ok(Self{requests:Some(tx),completed:done_rx,thread:Some(thread),busy:false,geometry:None,
            snapshot:serde_json::json!({"version":0,"batch_active":false,"escape":null,"sources":[],"segments":[]})})
    }
}
impl Drop for Worker {
    fn drop(&mut self){
        self.requests.take();
        // With at most one outstanding job there is space for its one reply;
        // no result-send deadlock during close. Joining never happens in Frame.
        if let Some(thread)=self.thread.take(){let _=thread.join();}
    }
}
pub(crate) fn advance_async(state:&mut State,geometry:&Arc<Value>,listener:&Value,sources:&Value,time:f64,cabinets:&[crate::audio::line_array::Cabinet])->Result<Value,String> {
    if state.terrain_worker.is_none(){state.terrain_worker=Some(Worker::start()?);}
    let worker=state.terrain_worker.as_mut().unwrap();
    match worker.completed.try_recv(){
        Ok(result)=>{
            worker.busy=false;
            let result=result?;
            worker.snapshot=result.snapshot;
            state.terrain_geometry=Some(result.geometry);
            state.terrain_rig=result.rig;
            state.engine.responses=result.responses;state.engine.debug=result.segments;
            state.engine.version=result.version;state.engine.escape=result.escape;
        },
        Err(mpsc::TryRecvError::Empty)=>{},
        Err(mpsc::TryRecvError::Disconnected)=>return Err("terrain acoustic worker terminated".into()),
    }
    if !worker.busy {
        let revision=geometry["revision"].as_u64().ok_or("geometry revision required")?;
        let retired=if worker.geometry.as_ref().is_none_or(|g|g["revision"].as_u64()!=Some(revision)) {
            worker.geometry.replace(geometry.clone())
        }else{None};
        worker.requests.as_ref().ok_or("terrain acoustic worker closed")?.try_send(Request{
            geometry:worker.geometry.as_ref().unwrap().clone(),listener:listener.clone(),sources:sources.clone(),time,
            cabinets:cabinets.iter().map(|c|c.position).collect(),retired
        }).map_err(|e|format!("terrain acoustic queue: {e}"))?;
        worker.busy=true;
    }
    Ok(worker.snapshot.clone())
}

#[cfg(test)]mod worker_tests {
 use super::*;
 fn geometry()->Value {serde_json::json!({"revision":1,"origin":[0.,crate::terrain::RADIUS,0.],"terrain":{"edits":[]},"underground":null,"triangles":[],"solids":[],"tetrahedra":[],"wood":[],"venue":null})}
 #[test]fn off_thread_acoustics_matches_the_same_synchronous_batch_step(){
  let geometry=Arc::new(geometry());
  let listener=serde_json::json!({"position":[0.,crate::terrain::RADIUS+10.,0.],"forward":[0.,0.,1.],"up":[0.,1.,0.]});
  let sources=serde_json::json!([{"id":1,"channel":0,"position":[2.,crate::terrain::RADIUS+10.,0.]}]);
  let expected=advance_parts(&mut State::default(),&geometry,&listener,&sources,0.).unwrap();
  let worker=Worker::start().unwrap();
  worker.requests.as_ref().unwrap().send(Request{geometry,listener,sources,time:0.,cabinets:vec![],retired:None}).unwrap();
  let got=worker.completed.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
  assert_eq!(got.snapshot,expected);assert_eq!(got.geometry.revision(),1);
 }
 #[test]fn worker_reports_bad_geometry_and_joins_with_a_pending_reply(){
  let worker=Worker::start().unwrap();
  worker.requests.as_ref().unwrap().send(Request{geometry:Arc::new(Value::Null),listener:Value::Null,sources:Value::Null,time:0.,cabinets:vec![],retired:None}).unwrap();
  assert!(worker.completed.recv_timeout(std::time::Duration::from_secs(5)).unwrap().is_err());
  worker.requests.as_ref().unwrap().send(Request{geometry:Arc::new(Value::Null),listener:Value::Null,sources:Value::Null,time:0.,cabinets:vec![],retired:None}).unwrap();
  drop(worker);
 }
}

#[cfg(test)]
mod wind_cover_tests {
    use super::*;
    use crate::audio::{V, geometry::{Hit, Snapshot}};
    use serde_json::json;
    fn roofs() -> Snapshot {
        Snapshot::parse(&json!({"revision":1,"origin":[0.,0.,0.],"triangles":[],
            "solids":[{"min":[-100.,3.,-100.],"max":[100.,4.,100.]},
                      {"min":[-100.,8.,-100.],"max":[100.,10.,100.]}]})).unwrap()
    }
    #[test]
    fn wind_clears_highest_roof_and_eases_back_outdoors() {
        let geometry=roofs(); let mut cover=WindCover::default();
        let up=[0.,1.,0.];
        assert_eq!(cover.position(&geometry,[0.,0.,0.],up,0.),[0.,11.,0.]);
        let edge=cover.position(&geometry,[200.,0.,0.],up,0.2);
        assert!(edge[1]>2. && edge[1]<11.);
        let falling=cover.position(&geometry,[200.,0.,0.],up,0.8);
        assert!(falling[1]>2. && falling[1]<11.);
        for n in 1..80 {cover.position(&geometry,[200.,0.,0.],up,0.8+n as f64*0.1);}
        assert!((cover.offset[1]-2.).abs()<1e-6);
    }
    #[test]
    fn underground_and_microphone_have_independent_cover() {
        let geometry=roofs(); let up=[0.,1.,0.];
        let mut player=WindCover::default(); let mut microphone=WindCover::default();
        assert_eq!(player.position(&geometry,[0.,-20.,0.],up,0.),[0.,11.,0.]);
        assert_eq!(microphone.position(&geometry,[200.,0.,0.],up,0.),[200.,2.,0.]);
    }
    fn room(open:bool)->Snapshot {
        let mut solids=vec![
            json!({"min":[-100.,5.,-8.],"max":[0.,6.,8.]}),
            json!({"min":[-100.,-1.,-8.],"max":[0.,0.,8.]}),
            json!({"min":[-101.,0.,-8.],"max":[-100.,6.,8.]}),
            json!({"min":[-100.,0.,-9.],"max":[0.,6.,-8.]}),
            json!({"min":[-100.,0.,8.],"max":[0.,6.,9.]}),
            json!({"min":[0.,0.,-8.],"max":[0.2,6.,-1.]}),
            json!({"min":[0.,0.,1.],"max":[0.2,6.,8.]}),
            json!({"min":[0.,3.,-1.],"max":[0.2,6.,1.]})];
        if !open {solids.push(json!({"min":[0.,0.,-1.],"max":[0.2,3.,1.]}));}
        Snapshot::parse(&json!({"revision":1,"origin":[0.,0.,0.],"triangles":[],"solids":solids})).unwrap()
    }
    #[test]
    fn open_door_draws_wind_down_toward_outside_but_closed_door_and_deep_room_do_not() {
        let up=[0.,1.,0.];let near=[-0.4,1.6,0.];
        let mut door=WindCover::default();let a=door.position(&room(true),near,up,0.);
        let mut closed=WindCover::default();let b=closed.position(&room(false),near,up,0.);
        assert!(a[0]>near[0]+0.2 && a[1]<b[1]-0.5,"{a:?} {b:?}");
        assert_eq!(closed.openness,0.);assert!(b[1]>=7.-1e-6);
        let mut deep=WindCover::default();deep.position(&room(true),[-80.,1.6,0.],up,0.);
        assert_eq!(deep.openness,0.);
    }
    #[test]
    fn closing_door_eases_pose_without_an_instant_threshold_jump() {
        let mut cover=WindCover::default();let p=[-0.4,1.6,0.];let up=[0.,1.,0.];
        let a=cover.position(&room(true),p,up,0.);
        let b=cover.position(&room(false),p,up,0.2);
        assert!(b[1]>a[1] && b[1]<7.);
        for n in 1..60 {cover.position(&room(false),p,up,0.2+n as f64*0.1);}
        assert!((cover.offset[1]-5.4).abs()<1e-5);
    }
    #[test]
    fn wind_uses_local_up_and_bounded_probes() {
        struct Ceiling;
        impl Geometry for Ceiling {
            fn revision(&self)->u64 {0}
            fn trace(&self,o:V,d:V,limit:f64)->Option<Hit> {
                if limit==64. {return Some(Hit{position:crate::audio::add(o,crate::audio::mul(d,1.)),normal:crate::audio::mul(d,-1.),distance:1.});}
                assert_eq!(o,[128.,0.,0.]); assert_eq!(d,[-1.,0.,0.]); assert!(limit<128.);
                Some(Hit{position:[10.,0.,0.],normal:[1.,0.,0.],distance:118.})
            }
            fn wall_distance(&self,_:V,_:V,_:f64)->f64 {0.}
        }
        assert_eq!(WindCover::default().position(&Ceiling,[0.;3],[1.,0.,0.],0.),[11.,0.,0.]);
    }
}

//! Position-driven collision refresh from immutable accepted terrain inputs.
//! Existing coverage remains usable while a new patch builds. World edits and
//! missing coverage wait for the matching snapshot; stale material never wins.
use super::{D, water_snapshot::Cache};
use crate::water::types::Snapshot;
use serde_json::Value;
use std::sync::mpsc;
pub(super) struct Input {
    pub position: D,
    pub frame: Value,
    pub material: Vec<[f64; 3]>,
    pub architecture:std::sync::Arc<Vec<D>>,
    pub architecture_revision:u64,
    pub architecture_focus:Option<D>,
}
#[derive(Clone,Default,PartialEq,Eq)]
struct Key {world:Vec<u8>,material:Vec<[u64;3]>,architecture_revision:u64}
impl Input {
    fn key(&self)->Result<Key,String>{
        // Exact identity, including signed zero, without formatting every
        // static architecture coordinate as decimal JSON on every frame.
        Ok(Key{world:serde_json::to_vec(&(&self.frame["edits"],&self.frame["debris"],&self.frame["underground"])).map_err(|e|e.to_string())?,
            material:self.material.iter().map(|p|p.map(f64::to_bits)).collect(),architecture_revision:self.architecture_revision})
    }
}
pub(crate) struct Worker {
    input: Option<mpsc::SyncSender<Input>>,
    output: mpsc::Receiver<Result<Snapshot, String>>,
    thread: Option<std::thread::JoinHandle<()>>,
    pending: bool,
    pending_key: Key,
    accepted_key: Key,
    accepted: Option<Snapshot>,
    pending_architecture:Option<D>,
    accepted_architecture:Option<D>,
    pending_position:Option<[u64;3]>,accepted_position:Option<[u64;3]>,
}
impl Worker {
    pub(super) fn start() -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<Input>(1);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("mirror-water-collision".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let mut cache = Cache::default();
                while let Ok(input) = rx.recv() {
                    let _scope = crate::host::performance_telemetry::Scope::new(
                        crate::host::performance_telemetry::Category::GameLogic,
                    );
                    let mut material=input.material;material.extend_from_slice(&input.architecture);
                    let result = cache.capture_numeric_material(
                        input.position,
                        &input.frame,
                        &Value::Null,
                        material,
                    );
                    if done_tx.send(result).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("water collision worker: {e}"))?;
        Ok(Self {
            input: Some(tx),
            output: done_rx,
            thread: Some(thread),
            pending: false,
            pending_key: Key::default(),
            accepted_key: Key::default(),
            accepted: None,
            pending_architecture:None,accepted_architecture:None,pending_position:None,accepted_position:None,
        })
    }
    fn accept(&mut self, result: Result<Snapshot, String>) -> Result<(), String> {
        self.pending = false;
        let snapshot = result?;
        self.accepted = Some(snapshot);
        self.accepted_key = std::mem::take(&mut self.pending_key);
        self.accepted_architecture=self.pending_architecture.take();self.accepted_position=self.pending_position.take();
        Ok(())
    }
    fn wait(&mut self) -> Result<(), String> {
        let result = self
            .output
            .recv()
            .map_err(|_| "water collision worker terminated".to_owned())?;
        self.accept(result)
    }
    pub(super) fn advance(&mut self, input: Input) -> Result<Snapshot, String> {
        if self.pending {
            match self.output.try_recv() {
                Ok(result) => self.accept(result)?,
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("water collision worker terminated".into());
                }
            }
        }
        let key = input.key()?;
        let usable=|worker:&Self|worker.accepted.as_ref().is_some_and(|s|s.contains(input.position,0.4))&&worker.accepted_key==key
            &&worker.accepted_architecture.is_none_or(|p|(0..3).all(|i|(input.position[i]-p[i]).abs()+0.4<=8.));
        if self.pending&&!usable(self){self.wait()?;}
        // A pending prefetch may just have supplied the matching coverage.
        // Re-check it before deciding whether another synchronous wait is needed.
        let valid=usable(self);
        let unchanged=valid&&self.accepted_position==Some(input.position.map(f64::to_bits))&&self.accepted_architecture==input.architecture_focus;
        if !self.pending&&!unchanged {
            self.pending_position=Some(input.position.map(f64::to_bits));
            self.pending_architecture=input.architecture_focus;
            self.input
                .as_ref()
                .unwrap()
                .try_send(input)
                .map_err(|e| format!("water collision queue: {e}"))?;
            self.pending = true;
            self.pending_key = key.clone();
        }
        if !valid {
            // A previous pending request may have had an older world key. In that
            // case the request above carries this exact edit; wait for it explicitly.
            self.wait()?;
            if self.accepted_key != key {
                return Err("water collision world revision mismatch".into());
            }
        }
        self.accepted
            .clone()
            .ok_or_else(|| "water collision snapshot missing".into())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Input {
        Input {
            position: [0., crate::terrain::RADIUS + 2., 0.],
            frame: serde_json::json!({"edits":[],"debris":[],"underground":null}),
            material: vec![],architecture:Default::default(),architecture_revision:0,architecture_focus:None,
        }
    }
    #[test]
    fn background_collision_is_exact_and_world_changes_cannot_publish_stale_material() {
        let mut cache = Cache::default();
        let a = input();
        let expected = cache
            .capture_numeric_material(a.position, &a.frame, &Value::Null, a.material)
            .unwrap();
        let mut worker = Worker::start().unwrap();
        let got = worker.advance(input()).unwrap();
        assert_eq!(got.packet(), expected.packet());
        let _ = worker.advance(input()).unwrap();
        let mut changed = input();
        changed.material = vec![
            [0., crate::terrain::RADIUS + 1., 0.],
            [1., crate::terrain::RADIUS + 1., 0.],
            [0., crate::terrain::RADIUS + 1., 1.],
        ];
        let updated = worker.advance(changed).unwrap();
        assert!(updated.revision > got.revision);
        assert_eq!(updated.triangles.len(), got.triangles.len() + 1);
    }
}

#[cfg(test)]mod key_tests {
 use super::*;
 #[test]fn numeric_collision_key_preserves_world_and_material_identity(){
  let mut input=Input{position:[0.,6_371_000.,0.],frame:serde_json::json!({"edits":[],"debris":[],"underground":null}),material:vec![[0.,1.,2.];60000],architecture:Default::default(),architecture_revision:0,architecture_focus:None};
  let first=input.key().unwrap();input.position[0]+=12.;assert!(input.key().unwrap()==first);
  input.material[0][0]=-0.;assert!(input.key().unwrap()!=first);input.material[0][0]=0.;assert!(input.key().unwrap()==first);
  input.frame["edits"]=serde_json::json!([{"change":1}]);assert!(input.key().unwrap()!=first);
  let start=std::time::Instant::now();std::hint::black_box(serde_json::to_vec(&(&input.frame["edits"],&input.frame["debris"],&input.frame["underground"],&input.material)).unwrap());let json=start.elapsed();
  let start=std::time::Instant::now();std::hint::black_box(input.key().unwrap());eprintln!("collision identity: JSON {:?}, numeric {:?}",json,start.elapsed());
 }
}

#[cfg(test)]mod architecture_guard_tests {
 use super::*;
 fn request(x:f64,focus:f64,revision:u64,height:f64)->Input{
  let r=crate::terrain::RADIUS;Input{position:[x,r+2.,0.],frame:serde_json::json!({"edits":[],"debris":[],"underground":null}),material:Vec::new(),architecture:std::sync::Arc::new(vec![[45.,r+height,0.],[46.,r+height,0.],[45.,r+height,1.]]),architecture_revision:revision,architecture_focus:Some([focus,r+2.,0.])}
 }
 #[test]fn architecture_guard_waits_for_missing_coverage_and_rejects_old_asset_revision(){
  let mut worker=Worker::start().unwrap();let first=worker.advance(request(0.,0.,1,12.)).unwrap();
  let again=worker.advance(request(0.,0.,1,12.)).unwrap();assert_eq!(first.packet(),again.packet());assert!(!worker.pending,"unchanged requests must not launch duplicate builds");
  let _=worker.advance(request(4.,4.,1,12.)).unwrap();
  let outside=worker.advance(request(24.,24.,1,13.)).unwrap();assert_eq!(worker.accepted_architecture,Some([24.,crate::terrain::RADIUS+2.,0.]));
  assert!(outside.vertices.iter().any(|v|(outside.origin[1]+v[1]as f64-crate::terrain::RADIUS-13.).abs()<0.01));
  let changed=worker.advance(request(24.,24.,2,20.)).unwrap();assert!(changed.vertices.iter().any(|v|(changed.origin[1]+v[1]as f64-crate::terrain::RADIUS-20.).abs()<0.01));assert!(!changed.vertices.iter().any(|v|(changed.origin[1]+v[1]as f64-crate::terrain::RADIUS-13.).abs()<0.01));
 }
}

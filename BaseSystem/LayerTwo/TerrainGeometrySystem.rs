//! Contracted immutable query data from the accepted planet state.
use crate::{
    audio::{geometry::Geometry, sub, vector},
    host::Host,
};
use serde_json::{Value, json};
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "Capture" => capture_inputs(host,input)?.build(),

        "Trace" => {
            let surface = crate::terrain::material_geometry::Surface::parse(&input["snapshot"])?;
            let distance = input["distance"]
                .as_f64()
                .filter(|d| d.is_finite() && (0.0..=512.).contains(d))
                .ok_or("trace distance must be 0..512 metres")?;
            Ok(surface.trace(vector(&input["position"])?,vector(&input["direction"])?,distance)
                .map_or(Value::Null,|hit|json!({"position":hit.position,"normal":hit.normal,"distance":hit.distance})))
        }
        "MaterialIntervals" => {
            let surface = crate::terrain::material_geometry::Surface::parse(&input["snapshot"])?;
            let rows = surface.intervals(vector(&input["start"])?, vector(&input["end"])?)?;
            let distance: f64 = rows.iter().map(|r| r[1] - r[0]).sum();
            Ok(json!({"intervals":rows,"solid_distance":distance,"resolution_metres":0.05}))
        }
        _ => Err("unknown terrain geometry hook".into()),
    }
}

/// Owned accepted data only. Expensive tree sampling, hashing and JSON assembly
/// can run off the game thread without reading a mutable Host or live world.
pub(crate) struct Capture {
    position:[f64;3],time:f64,soil:Value,frame:Value,
    plants:Option<crate::vegetation::State>,venue:Value,cells:Vec<[[f64;3];4]>,
}
pub(crate) fn capture_inputs(host:&mut Host,input:&Value)->Result<Capture,String> {
    let position=vector(&input["position"])?;
    if crate::audio::length(position)<1.{return Err("planet position required".into());}
    let soil=if host.system_enabled("TerrainToolSystem"){host.invoke("TerrainToolSystem.Frame",Value::Null)?}else{Value::Null};
    let plants=if host.system_enabled("VegetationSystem"){Some(host.acoustic_plant_snapshot()?)}else{None};
    let frame=host.invoke("PlanetSurfaceSystem.Frame",Value::Null)?;
    let venue=if host.system_enabled("TheatreSystem"){host.invoke("TheatreSystem.AcousticGeometry",json!({"position":position}))?}else{Value::Null};
    Ok(Capture{position,time:input["time"].as_f64().unwrap_or(0.),soil,frame,plants,venue,cells:host.planet.acoustic_cells()})
}
impl Capture {
 pub(crate) fn build(self)->Result<Value,String> {
    let Self{position,time,soil,frame,plants:state,venue,cells}=self;
    let normal=crate::terrain::mesh_anchor(position);
    let origin=normal.map(|n|n*crate::terrain::RADIUS);
    let plants=if let Some(state)=state {
        crate::base_system::layer_two::vegetation_system::query_wood(&state,&json!({"position":origin,"radius":128.,"time":time}))?
    }else{json!({"wood":[]})};
            let mut tetrahedra = Vec::<[[f64; 3]; 4]>::new();
            if let Some(rows) = soil["tetrahedra"].as_array() {
                for row in rows {
                    let points: [[f64; 3]; 4] = serde_json::from_value(row.clone())
                        .map_err(|_| "accepted tetrahedron required")?;
                    tetrahedra.push(points.map(|p| sub(p, origin)));
                }
            }
            let vertices = frame["debris"]
                .as_array()
                .ok_or("accepted debris required")?;
            if vertices.len() % 3 != 0 || vertices.len() > 32768 * 3 {
                return Err("accepted debris triangle budget exceeded".into());
            }
            let mut triangles = Vec::new();
            for face in vertices.chunks_exact(3) {
                let mut triangle = [[0.; 3]; 3];
                for (i, v) in face.iter().enumerate() {
                    let p = [
                        v[0].as_f64().ok_or("accepted vertex required")?,
                        v[1].as_f64().ok_or("accepted vertex required")?,
                        v[2].as_f64().ok_or("accepted vertex required")?,
                    ];
                    triangle[i] = sub(p, origin);
                }
                triangles.push(triangle);
            }
            for cell in cells {
                tetrahedra.push(cell.map(|point| sub(point, origin)));
            }
            if tetrahedra.len() > 4096 {
                return Err("accepted material tetrahedron budget exceeded".into());
            }
            // Revision depends on accepted geometry, never camera orientation.
            let bytes = serde_json::to_vec(&json!([
                frame["edits"],
                frame["debris"],
                tetrahedra,
                plants["wood"],
                frame["underground"],
                venue["revision"]
            ]))
            .map_err(|e| e.to_string())?;
            let mut revision = 0xcbf29ce484222325u64;
            for byte in bytes {
                revision = (revision ^ byte as u64).wrapping_mul(0x100000001b3);
            }
            Ok(
                json!({"revision":revision,"origin":origin,"terrain":{"edits":frame["edits"]},"underground":frame["underground"],"triangles":triangles,"solids":[],"venue":venue,"material_resolution_metres":0.05,"tetrahedra":tetrahedra,"wood":plants["wood"],"solid_scope":["continuous_terrain","accepted_soil_tetrahedra","living_wood","authored_theatre_prisms"],"surface_scope":"accepted_planet_debris_and_theatre"}),
            )
 }
}

/// One immutable geometry build in flight; publication is polled by the owning
/// environment Audicle. Closing joins the worker and drops unpublished data.
pub(crate) struct Worker {
    input:Option<std::sync::mpsc::SyncSender<Capture>>,
    output:std::sync::mpsc::Receiver<Result<Value,String>>,
    thread:Option<std::thread::JoinHandle<()>>,
    pub(crate) busy:bool,
}
impl Worker {
    pub(crate) fn start()->Result<Self,String> {
        let(tx,rx)=std::sync::mpsc::sync_channel::<Capture>(1);
        let(done_tx,done_rx)=std::sync::mpsc::sync_channel(1);
        let thread=std::thread::Builder::new().name("mirror-acoustic-geometry".into()).spawn(move||{
            while let Ok(input)=rx.recv(){
                let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::Audio);
                if done_tx.send(input.build()).is_err(){break;}
            }
        }).map_err(|e|format!("acoustic geometry worker: {e}"))?;
        Ok(Self{input:Some(tx),output:done_rx,thread:Some(thread),busy:false})
    }
    pub(crate) fn submit(&mut self,input:Capture)->Result<(),String>{
        self.input.as_ref().ok_or("acoustic geometry worker closed")?.try_send(input).map_err(|e|format!("acoustic capture queue: {e}"))?;
        self.busy=true;Ok(())
    }
    pub(crate) fn poll(&mut self)->Result<Option<Value>,String>{
        match self.output.try_recv(){
            Ok(result)=>{self.busy=false;result.map(Some)},
            Err(std::sync::mpsc::TryRecvError::Empty)=>Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected)=>Err("acoustic geometry worker terminated".into()),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self){self.input.take();if let Some(thread)=self.thread.take(){let _=thread.join();}}
}

#[cfg(test)]mod worker_tests {
 use super::*;
 fn input()->Capture{Capture{position:[0.,crate::terrain::RADIUS+4.,0.],time:0.,soil:Value::Null,
  frame:json!({"edits":[],"debris":[],"underground":null}),plants:None,venue:Value::Null,cells:vec![]}}
 #[test]fn captured_geometry_builds_identically_off_thread(){
  let expected=input().build().unwrap();let mut worker=Worker::start().unwrap();worker.submit(input()).unwrap();
  let result=worker.output.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();assert_eq!(result,expected);
  crate::terrain::material_geometry::Surface::parse(&result).unwrap();
 }
}

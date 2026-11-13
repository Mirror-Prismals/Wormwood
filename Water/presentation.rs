//! Immutable numeric water attributes shared by Entity snapshots and native scenes.
//! JSON is an explicit compatibility adapter, never the live mesh transport.
use super::math::Frame;
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub frame: Frame,
    pub rows: Arc<Vec<[[f64; 4]; 6]>>,
    pub indices: Arc<Vec<u32>>,
    pub optics: Value,
    pub ripples: Value,
}
impl Packet {
    /// Deep terrain omits the exterior height field. Surface pools cannot be
    /// drawn through that missing domain; crossing descent sheets remain.
    pub(crate) fn retain_deep_surfaces(&mut self) {
        let indices=self.indices.chunks_exact(3).filter(|tri|tri.iter().any(|&i|{
            let p=self.rows[i as usize][0];
            let world=std::array::from_fn(|k|self.frame.origin[k]+p[k]);
            crate::terrain::underground::local(world)[1]< -18.
        })).flat_map(|tri|tri.iter().copied()).collect();
        self.indices=Arc::new(indices);
    }
    pub fn metadata(&self) -> Value {
        json!({"frame":{"origin":self.frame.origin,"right":self.frame.right,"up":self.frame.up,"forward":self.frame.forward},"optics":self.optics,"ripples":self.ripples})
    }
    pub fn json(&self) -> Value {
        let mut p = self.metadata();
        p["water_vertices"] = Value::Array(
            self.rows
                .iter()
                .map(|r| {
                    json!([
                        r[0],
                        r[1],
                        r[2],
                        r[3],
                        r[4],
                        [
                            json!(r[5][0]),
                            json!(r[5][1]),
                            json!(r[5][2] as u32),
                            json!(r[5][3] as u32)
                        ]
                    ])
                })
                .collect(),
        );
        p["water_indices"] = json!(self.indices.as_ref());
        p
    }
    pub fn parse(p: &Value) -> Result<Self, String> {
        let vector = |k: &str| {
            serde_json::from_value(p["frame"][k].clone())
                .map_err(|_| format!("water frame {k} required"))
        };
        Ok(Self {
            frame: Frame {
                origin: vector("origin")?,
                right: vector("right")?,
                up: vector("up")?,
                forward: vector("forward")?,
            },
            rows: Arc::new(
                serde_json::from_value(p["water_vertices"].clone())
                    .map_err(|_| "water rows required")?,
            ),
            indices: Arc::new(
                serde_json::from_value(p["water_indices"].clone())
                    .map_err(|_| "water indices required")?,
            ),
            optics: p["optics"].clone(),
            ripples: p["ripples"].clone(),
        })
    }
}
pub(crate) fn parse_collection(p: &Value) -> Result<Vec<Packet>, String> {
    if p.is_null() {
        return Ok(Vec::new());
    }
    p.as_array()
        .filter(|a| a.len() <= 8)
        .ok_or("bounded water packet array required")?
        .iter()
        .map(Packet::parse)
        .collect()
}
pub(crate) fn mesh_rows(frame: Frame, mesh: &super::surface_mesh::Mesh) -> Vec<[[f64; 4]; 6]> {
    mesh.vertices
        .iter()
        .map(|p| {
            let a = super::math::sub(p.position, frame.origin);
            [
                [a[0], a[1], a[2], 1.],
                [p.normal[0], p.normal[1], p.normal[2], 1.-crate::terrain::underground::sky_exposure(p.position)],
                [p.geometric[0], p.geometric[1], p.geometric[2], 0.],
                [p.axis[0], p.axis[1], p.axis[2], 0.],
                [p.uv[0], p.uv[1], p.flow[0], p.flow[1]],
                [p.depth, p.thickness, p.class as f64, p.flags as f64],
            ]
        })
        .collect()
}

#[cfg(test)] mod cave_water_tests {
 use super::*;
 #[test] fn cave_water_carries_roof_cover_without_changing_geometry() {
  let frame=super::super::surface_mesh::chart();
  let vertices=[0.,-44.].map(|height|super::super::surface_mesh::Vertex {
   position:crate::terrain::underground::world([-51.,height,16.]),normal:[0.,1.,0.],geometric:[0.,1.,0.],axis:[0.,1.,0.],uv:[0.;2],flow:[0.;2],depth:2.,thickness:2.,class:2,flags:2
  });
  let mesh=super::super::surface_mesh::Mesh{vertices:vertices.to_vec(),..Default::default()};
  let rows=mesh_rows(frame,&mesh);assert_eq!(rows[1][1][3],1.);
  for i in 0..2 {for k in 0..3 {assert_eq!(rows[i][0][k],vertices[i].position[k]-frame.origin[k]);}}
 }
 #[test] fn cave_water_shader_validates_with_roof_and_inspection_varyings() {
  let source=include_str!("../Procedures/Water/Shaders/water.wgsl");
  let module=naga::front::wgsl::parse_str(source).unwrap();
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
 }
}

#[cfg(test)] mod deep_surface_visibility_tests {
 use super::*;
 #[test] fn deep_view_excludes_surface_pools_but_retains_cave_and_crossing_water() {
  let frame=super::super::surface_mesh::chart();
  let rows=[0.,0.,0.,-40.,-40.,-40.,0.,-40.,0.].map(|h| {
   let p=crate::terrain::underground::world([0.,h,0.]);
   let mut row=[[0.;4];6];row[0]=[p[0]-frame.origin[0],p[1]-frame.origin[1],p[2]-frame.origin[2],1.];row
  });
  let mut p=Packet{frame,rows:Arc::new(rows.to_vec()),indices:Arc::new((0..9).collect()),optics:Value::Null,ripples:Value::Null};
  let original=p.clone();p.retain_deep_surfaces();
  assert_eq!(*p.indices,vec![3,4,5,6,7,8]);assert_eq!(original.indices.len(),9);
 }
}

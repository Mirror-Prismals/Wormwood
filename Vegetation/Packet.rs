//! Typed boundary from QueryGeometry to native indexed material/shadow passes.
//! Matrices and lighting are supplied by the shared scene owner; no projection,
//! light direction or shadow map is invented by this adapter.
use serde_json::Value;

pub const VERTEX_STRIDE: usize = 64;
pub const SCENE_BYTES: usize = 192;
pub type Matrix = [[f32; 4]; 4];

/// Numeric equivalent of QueryGeometry's public JSON envelope. Keep large
/// vertex arrays out of serde during live rendering; diagnostic callers retain
/// the JSON representation and the same named System route.
pub(crate) struct Geometry {
    pub origin: [f64; 3],
    pub revision: u64,
    pub vertices: Vec<[f32; 16]>,
    pub indices: Vec<u32>,
    pub metadata: Value,
}
impl Geometry {
    pub fn into_json(self) -> Value {
        let mut value = self.metadata;
        value["origin"] = serde_json::json!(self.origin);
        value["revision"] = serde_json::json!(self.revision);
        value["vertex_stride"] = serde_json::json!(16);
        value["vertices"] = serde_json::json!(self.vertices);
        value["indices"] = serde_json::json!(self.indices);
        value
    }
}

pub struct Scene {
    /// Column-major canonical 0..1 clip-depth matrices, same relative origin.
    pub view_projection: Matrix,
    pub sun_view_projection: Matrix,
    pub sun_direction: [f32; 4],
    pub sun_color: [f32; 4],
    pub ambient_color: [f32; 4],
    /// options[0] enables sunlight shadows; other words reserved, set to zero.
    pub options: [u32; 4],
}
impl Scene {
    pub fn from_view(view: &Value) -> Result<Self, String> {
        use super::math::*;
        let (sy, cy) = view["yaw"].as_f64().ok_or("scene yaw required")?.sin_cos();
        let (sp, cp) = view["pitch"]
            .as_f64()
            .ok_or("scene pitch required")?
            .sin_cos();
        let axis = |key: &str, fallback: V| -> Result<V, String> {
            if view[key].is_null() {
                Ok(fallback)
            } else {
                serde_json::from_value(view[key].clone())
                    .map_err(|_| format!("invalid scene {key}"))
            }
        };
        let right = axis("right", [cy, 0., sy])?;
        let up = axis("up", [-sy * sp, cp, cy * sp])?;
        let forward = axis("forward", [sy * cp, sp, -cy * cp])?;
        let focal = 1. / 35f64.to_radians().tan();
        let aspect = view["aspect"].as_f64().unwrap_or(4. / 3.);
        let position: V = serde_json::from_value(view["position"].clone())
            .map_err(|_| "scene origin required")?;
        let near = 0.05;
        let far = length(position) + 2. * crate::terrain::RADIUS;
        let view_projection = std::array::from_fn(|c| {
            if c < 3 {
                [
                    (right[c] * focal / aspect) as f32,
                    (up[c] * focal) as f32,
                    (forward[c] * far / (far - near)) as f32,
                    forward[c] as f32,
                ]
            } else {
                [0., 0., (-far * near / (far - near)) as f32, 0.]
            }
        });
        let lighting = crate::base_system::layer_four::atmosphere::Lighting::from_view(view);
        let sun = lighting.primary;
        let (east, north) = basis(sun);
        // A bounded directional-light orthographic volume in the same floating origin.
        let centre = mul(forward, 40.);
        let half = 160.;
        let depth = 640.;
        let sun_view_projection = std::array::from_fn(|c| {
            if c < 3 {
                [
                    (east[c] / half) as f32,
                    (north[c] / half) as f32,
                    (-sun[c] / depth) as f32,
                    0.,
                ]
            } else {
                [
                    (-dot(east, centre) / half) as f32,
                    (-dot(north, centre) / half) as f32,
                    (0.5 + dot(sun, centre) / depth) as f32,
                    1.,
                ]
            }
        });
        let scene = Self {
            view_projection,
            sun_view_projection,
            sun_direction: [sun[0] as f32, sun[1] as f32, sun[2] as f32, 0.],
            sun_color: {
                let c = lighting.sun_color();
                [c[0] as f32, c[1] as f32, c[2] as f32, 1.]
            },
            ambient_color: {
                let c = lighting.ambient();
                [c[0] as f32, c[1] as f32, c[2] as f32, 1.]
            },
            options: [1, 0, 0, 0],
        };
        scene.validate()?;
        Ok(scene)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self
            .view_projection
            .iter()
            .flatten()
            .chain(self.sun_view_projection.iter().flatten())
            .chain(self.sun_direction.iter())
            .chain(self.sun_color.iter())
            .chain(self.ambient_color.iter())
            .all(|v| v.is_finite())
            || self.options[0] > 1
            || self.options[1..].iter().any(|v| *v != 0)
            || self.sun_direction[..3].iter().map(|v| v * v).sum::<f32>() < 1e-12
        {
            return Err("invalid vegetation scene uniforms".into());
        }
        Ok(())
    }
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(SCENE_BYTES);
        for value in self
            .view_projection
            .iter()
            .flatten()
            .chain(self.sun_view_projection.iter().flatten())
            .chain(self.sun_direction.iter())
            .chain(self.sun_color.iter())
            .chain(self.ambient_color.iter())
        {
            bytes.extend(value.to_ne_bytes());
        }
        for word in self.options {
            bytes.extend(word.to_ne_bytes());
        }
        Ok(bytes)
    }
}

pub struct Packet {
    pub origin: [f64; 3],
    pub revision: u64,
    pub vertices: Vec<[f32; 16]>,
    pub indices: Vec<u32>,
    pub scene: Scene,
}
impl Packet {
    pub(crate) fn from_numeric(geometry: Geometry, scene: Scene) -> Result<Self, String> {
        let packet = Self {
            origin: geometry.origin,
            revision: geometry.revision,
            vertices: geometry.vertices,
            indices: geometry.indices,
            scene,
        };
        packet.validate()?;
        Ok(packet)
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.scene.validate()?;
        if !self.origin.iter().all(|v| v.is_finite() && v.abs() <= 1e9)
            || self.vertices.len() > 98304
            || self.indices.len() > 98304
            || self.indices.len() % 3 != 0
            || self
                .vertices
                .iter()
                .any(|v| !v.iter().all(|v| v.is_finite()) || ![0., 1., 2., 3., 4., 5., 6.].contains(&v[11]))
            || self
                .indices
                .iter()
                .any(|&i| i as usize >= self.vertices.len())
        {
            return Err("invalid bounded numeric vegetation packet".into());
        }
        Ok(())
    }
    /// The caller must construct Scene in the returned packet's exact origin.
    pub fn from_geometry(value: &Value, scene: Scene) -> Result<Self, String> {
        scene.validate()?;
        if value["vertex_stride"] != 16 {
            return Err("vegetation requires a 16-float vertex".into());
        }
        let origin: [f64; 3] = serde_json::from_value(value["origin"].clone())
            .map_err(|_| "vegetation origin required")?;
        if !origin.iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
            return Err("invalid vegetation origin".into());
        }
        let revision = value["revision"]
            .as_u64()
            .ok_or("vegetation revision required")?;
        let rows = value["vertices"]
            .as_array()
            .filter(|v| v.len() <= 98304)
            .ok_or("vegetation vertex budget exceeded")?;
        let index_rows = value["indices"]
            .as_array()
            .filter(|v| v.len() <= 98304 && v.len() % 3 == 0)
            .ok_or("invalid vegetation triangle budget")?;
        let vertices: Vec<[f32; 16]> = rows
            .iter()
            .map(|row| {
                let v: [f32; 16] =
                    serde_json::from_value(row.clone()).map_err(|_| "invalid vegetation vertex")?;
                if !v.iter().all(|v| v.is_finite()) || ![0., 1., 2., 3., 4., 5., 6.].contains(&v[11]) {
                    return Err("invalid vegetation vertex material/data");
                }
                Ok(v)
            })
            .collect::<Result<_, _>>()?;
        let indices = index_rows
            .iter()
            .map(|row| {
                row.as_u64()
                    .filter(|index| *index < vertices.len() as u64)
                    .map(|index| index as u32)
                    .ok_or("vegetation index out of bounds")
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            origin,
            revision,
            vertices,
            indices,
            scene,
        })
    }
    pub(crate) fn vertex_data(&self)->&[u8] {
        // Arrays of f32 are contiguous with no padding; the API copies these
        // native-endian bytes before returning. The borrow cannot outlive self.
        unsafe {std::slice::from_raw_parts(self.vertices.as_ptr().cast(),std::mem::size_of_val(self.vertices.as_slice()))}
    }
    pub(crate) fn index_data(&self)->&[u8] {
        unsafe {std::slice::from_raw_parts(self.indices.as_ptr().cast(),std::mem::size_of_val(self.indices.as_slice()))}
    }
    pub fn vertex_bytes(&self) -> Vec<u8> {
        self.vertices
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect()
    }
    pub fn index_bytes(&self) -> Vec<u8> {
        self.indices.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    pub(super) fn scene() -> Scene {
        let identity =
            std::array::from_fn(|c| std::array::from_fn(|r| if c == r { 1. } else { 0. }));
        Scene {
            view_projection: identity,
            sun_view_projection: identity,
            sun_direction: [0., 1., 0., 0.],
            sun_color: [1.; 4],
            ambient_color: [0.2; 4],
            options: [1, 0, 0, 0],
        }
    }
    #[test]
    fn native_packet_retains_indexed_layout_and_integer_shadow_option() {
        let mut vertex = [0f32; 16];
        vertex[4] = 1.;
        vertex[11] = 1.;
        let mut value = json!({"origin":[0.,1.,0.],"revision":7,"vertex_stride":16,"vertices":[vertex,vertex,vertex],"indices":[0,1,2]});
        let packet = Packet::from_geometry(&value, scene()).unwrap();
        assert_eq!(packet.vertex_bytes().len(), 3 * VERTEX_STRIDE);
        assert_eq!(packet.index_bytes().len(), 12);
        let uniforms = packet.scene.bytes().unwrap();
        assert_eq!(uniforms.len(), SCENE_BYTES);
        assert_eq!(&uniforms[176..180], &1u32.to_ne_bytes());
        value["indices"][2] = json!(3);
        assert!(Packet::from_geometry(&value, scene()).is_err());
        value["indices"] = json!([0, 1]);
        assert!(Packet::from_geometry(&value, scene()).is_err());
    }
    #[test]
    fn species_materials_cross_both_packet_boundaries_and_unknown_tags_fail() {
        for material in [0.,1.,2.,3.,4.,5.,6.,7.,2.5,-1.] {
            let mut vertex=[0f32;16];vertex[4]=1.;vertex[11]=material;
            let value=json!({"origin":[0.,0.,0.],"revision":1,"vertex_stride":16,"vertices":[vertex,vertex,vertex],"indices":[0,1,2]});
            let expected=[0.,1.,2.,3.,4.,5.,6.].contains(&material);
            assert_eq!(Packet::from_geometry(&value,scene()).is_ok(),expected,"JSON tag {material}");
            let geometry=Geometry{origin:[0.;3],revision:1,vertices:vec![vertex;3],indices:vec![0,1,2],metadata:json!({})};
            assert_eq!(Packet::from_numeric(geometry,scene()).is_ok(),expected,"numeric tag {material}");
        }
    }

}

#[cfg(test)]mod upload_byte_tests{
 use super::*;
 #[test]fn borrowed_upload_is_identical_to_original_native_byte_encoding(){
  let vertices=vec![[0.;16],[1.25;16],[-0.0;16]];
  let packet=Packet{origin:[0.;3],revision:0,vertices,indices:vec![0,1,2],scene:tests::scene()};
  assert_eq!(packet.vertex_data(),packet.vertex_bytes());assert_eq!(packet.index_data(),packet.index_bytes());
 }
}

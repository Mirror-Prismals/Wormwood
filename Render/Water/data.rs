//! Pure CPU water-pass ABI. Resource slots are logical; System JSON holds no GPU handles.
use serde_json::Value;
pub(crate) const MAX_VERTICES: usize = 196608;
pub(crate) const MAX_INDICES: usize = 393216;
pub(crate) const MAX_RIPPLES: usize = 64;
fn nonzero3(v: &[f32]) -> bool {
    v.len() >= 3
        && v[..3].iter().all(|n| n.is_finite())
        && v[..3].iter().map(|n| (*n as f64).powi(2)).sum::<f64>() > 1e-12
}
fn basis(u: &[f32; 3], v: &[f32; 3], w: &[f32; 3]) -> bool {
    let dot = |a: &[f32; 3], b: &[f32; 3]| (0..3).map(|i| a[i] as f64 * b[i] as f64).sum::<f64>();
    [u, v, w]
        .iter()
        .all(|a| nonzero3(*a) && (dot(*a, *a) - 1.).abs() < 0.02)
        && dot(u, v).abs() < 0.02
        && dot(u, w).abs() < 0.02
        && dot(v, w).abs() < 0.02
}
/// Six vec4s, 96-byte stride, shared by WGSL/std140-compatible native pipelines.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vertex {
    pub position: [f32; 4],
    pub shading_normal: [f32; 4],
    pub geometric_normal: [f32; 4],
    pub axis_normal: [f32; 4],
    pub uv_flow: [f32; 4],
    pub depth_class_flags: [f32; 4],
}
// Six initialized vec4 arrays, with no padding: the existing 96-byte shader ABI.
const _:()=assert!(std::mem::size_of::<Vertex>()==96&&std::mem::align_of::<Vertex>()==4);
impl Vertex {
    pub fn append_bytes(&self, out: &mut Vec<u8>) {
        for a in [
            &self.position,
            &self.shading_normal,
            &self.geometric_normal,
            &self.axis_normal,
            &self.uv_flow,
            &self.depth_class_flags,
        ] {
            for n in a {
                out.extend(n.to_ne_bytes());
            }
        }
    }
    pub fn valid(&self) -> bool {
        [
            self.position,
            self.shading_normal,
            self.geometric_normal,
            self.axis_normal,
            self.uv_flow,
            self.depth_class_flags,
        ]
        .iter()
        .flatten()
        .all(|n| n.is_finite())
            && self.depth_class_flags[0] >= 0.
            && self.depth_class_flags[1] >= 0.
            && nonzero3(&self.shading_normal)
            && nonzero3(&self.geometric_normal)
            && nonzero3(&self.axis_normal)
            && (0.0..=5.).contains(&self.depth_class_flags[2])
            && self.depth_class_flags[2].fract() == 0.
            && (0.0..=3.).contains(&self.depth_class_flags[3])
            && self.depth_class_flags[3].fract() == 0.
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Camera {
    pub origin: [f64; 3],
    pub view: [[f32; 4]; 4],
    pub projection: [[f32; 4]; 4],
    pub relative_position: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    pub width: u32,
    pub height: u32,
    pub near: f32,
    pub far: f32,
    pub reversed_z: bool,
}
impl Camera {
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= 16384
            && self.height <= 16384
            && self.near.is_finite()
            && self.near > 0.
            && self.far.is_finite()
            && self.far > self.near
            && self.origin.iter().all(|n| n.is_finite())
            && self.relative_position.iter().all(|n| n.is_finite())
            && basis(&self.right, &self.up, &self.forward)
            && self
                .view
                .iter()
                .flatten()
                .chain(self.projection.iter().flatten())
                .all(|n| n.is_finite())
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReflectionPlane {
    pub normal: [f64; 3],
    pub point: [f64; 3],
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ripple {
    pub uv: [f32; 2],
    pub age: f32,
    pub strength: f32,
}
#[derive(Clone, Debug)]
pub(crate) struct Optics {
    pub time: f32,
    pub murkiness: f32,
    pub light: [f32; 3],
    pub sky_top: [f32; 3],
    pub sky_bottom: [f32; 3],
    pub sky_color_intensity: f32,
    pub neutral_reflection_intensity: f32,
    pub planar_reflection: bool,
    pub fullbright: bool,
    pub cave_fog: bool,
    pub tangent_u: [f32; 3],
    pub tangent_v: [f32; 3],
    pub surface_up: [f32; 3],
    pub ripple_origin: [f32; 2],
    pub micro_phase: [f32; 3],
}
#[derive(Clone, Debug)]
pub(crate) struct DrawFrame {
    pub camera: Camera,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub optics: Optics,
    pub ripples: Vec<Ripple>,
    pub reflection_plane: Option<ReflectionPlane>,
}
impl DrawFrame {
    pub fn validate(&self) -> Result<(), String> {
        if !self.camera.valid() {
            return Err("invalid water camera/viewport".into());
        }
        if self.vertices.len() > MAX_VERTICES
            || self.indices.len() > MAX_INDICES
            || self.indices.len() % 3 != 0
            || self
                .indices
                .iter()
                .any(|i| *i as usize >= self.vertices.len())
            || self.vertices.iter().any(|v| !v.valid())
        {
            return Err("invalid bounded water mesh".into());
        }
        if self.ripples.len() > MAX_RIPPLES
            || self.ripples.iter().any(|r| {
                r.uv.iter().any(|n| !n.is_finite())
                    || !r.age.is_finite()
                    || r.age < 0.
                    || !r.strength.is_finite()
                    || r.strength < 0.
            })
        {
            return Err("invalid water ripples".into());
        }
        let o = &self.optics;
        if !o.time.is_finite()
            || !o.murkiness.is_finite()
            || !(0.0..=1.).contains(&o.murkiness)
            || !o.sky_color_intensity.is_finite()
            || !o.neutral_reflection_intensity.is_finite()
            || !(0.0..=1.).contains(&o.sky_color_intensity)
            || !(0.0..=1.).contains(&o.neutral_reflection_intensity)
            || !nonzero3(&o.light)
            || !basis(&o.tangent_u, &o.tangent_v, &o.surface_up)
            || o.ripple_origin.iter().any(|n| !n.is_finite())
            || o.micro_phase.iter().any(|n| !n.is_finite())
            || [
                o.light,
                o.sky_top,
                o.sky_bottom,
                o.tangent_u,
                o.tangent_v,
                o.surface_up,
            ]
            .iter()
            .flatten()
            .any(|n| !n.is_finite())
        {
            return Err("invalid water optical parameters".into());
        }
        if o.planar_reflection && self.reflection_plane.is_none() {
            return Err("water reflection requires a scene reflection plane".into());
        }
        if let Some(p) = self.reflection_plane {
            if p.point
                .iter()
                .chain(p.normal.iter())
                .any(|n| !n.is_finite())
                || p.normal.iter().map(|n| n * n).sum::<f64>() <= 1e-12
            {
                return Err("invalid water reflection plane".into());
            }
        }
        Ok(())
    }
    pub fn vertex_bytes_borrowed(&self)->&[u8]{
        // repr(C) + the size assertion above proves the identical initialized
        // native-endian layout. Upload APIs copy this immutable borrowed slice.
        unsafe{std::slice::from_raw_parts(self.vertices.as_ptr().cast(),std::mem::size_of_val(self.vertices.as_slice()))}
    }
    pub fn index_bytes_borrowed(&self)->&[u8]{
        unsafe{std::slice::from_raw_parts(self.indices.as_ptr().cast(),std::mem::size_of_val(self.indices.as_slice()))}
    }
    /// Retained serialization reference for ABI regression checks.
    pub fn vertex_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.vertices.len() * 96);
        for v in &self.vertices {
            v.append_bytes(&mut out);
        }
        out
    }
    pub fn index_bytes(&self) -> Vec<u8> {
        self.indices.iter().flat_map(|i| i.to_ne_bytes()).collect()
    }
}
/// Refraction must read the completed opaque scene. A water pass cannot sample
/// the color attachment it is currently writing. Reflection is a separate draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResourceSlot {
    SceneColor,
    SceneDepth,
    PlanarReflection,
    WaterLayer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    OpaqueScene,
    PlanarReflection,
    WaterSurface,
    Composite,
}
pub(crate) const PASS_ORDER: [Pass; 4] = [
    Pass::OpaqueScene,
    Pass::PlanarReflection,
    Pass::WaterSurface,
    Pass::Composite,
];
/// Preserve user-facing render failures rather than silently substituting opaque colors.
pub(crate) fn require_resources(
    resources: &[ResourceSlot],
    reflection: bool,
) -> Result<(), String> {
    for slot in [
        ResourceSlot::SceneColor,
        ResourceSlot::SceneDepth,
        ResourceSlot::WaterLayer,
    ] {
        if !resources.contains(&slot) {
            return Err(format!("water optics resource missing: {slot:?}"));
        }
    }
    if reflection && !resources.contains(&ResourceSlot::PlanarReflection) {
        return Err("water planar reflection target is missing".into());
    }
    Ok(())
}
pub(crate) fn logical_resource_report() -> Value {
    serde_json::json!({"scene_color":"completed opaque scene sampled during refraction","scene_depth":"opaque scene depth used by water depth testing","planar_reflection":"opaque scene drawn under reflected camera with plane clipping","water_layer":"separate water color/coverage target; composited after opaque scene"})
}

/// Validate/realize pure Entity attributes using the parent's backend-correct
/// camera. Entity positions are local to its f64 frame origin, then rebased.
pub(crate) fn draw_frame(packet: &Value, camera: Camera) -> Result<DrawFrame, String> {
    let rows: Vec<[[f32; 4]; 6]> = serde_json::from_value(packet["water_vertices"].clone())
        .map_err(|_| "invalid water attribute rows")?;
    if rows.len() > MAX_VERTICES {
        return Err("water attribute count exceeds budget".into());
    }
    let indices = serde_json::from_value(packet["water_indices"].clone()).map_err(|_| "water indices required")?;
    draw_parts(packet, camera, rows.into_iter(), indices)
}
pub(crate) fn draw_numeric(packet:&crate::water::presentation::Packet,camera:Camera)->Result<DrawFrame,String>{
    draw_parts(&packet.metadata(),camera,packet.rows.iter().map(|r|r.map(|a|a.map(|x|x as f32))),packet.indices.as_ref().clone())
}
fn draw_parts(packet:&Value,camera:Camera,rows:impl Iterator<Item=[[f32;4];6]>,indices:Vec<u32>)->Result<DrawFrame,String>{
    let vector3=|v:&Value|serde_json::from_value::<[f32;3]>(v.clone()).map_err(|_|"water vector required".to_owned());
    let origin:[f64;3]=serde_json::from_value(packet["frame"]["origin"].clone()).map_err(|_|"water origin required")?;
    let up=vector3(&packet["frame"]["up"])?;let tangent_u=vector3(&packet["frame"]["right"])?;let tangent_v=vector3(&packet["frame"]["forward"])?;
    let delta:[f64;3]=std::array::from_fn(|i|origin[i]-camera.origin[i]);
    let vertices = rows.map(|r| {let mut position=r[0];for i in 0..3{position[i]=(position[i]as f64+delta[i])as f32;}
        Vertex{position,shading_normal:r[1],geometric_normal:r[2],axis_normal:r[3],uv_flow:r[4],depth_class_flags:r[5]}}).collect();
    let o = &packet["optics"];
    let n = |key: &str| {
        o[key]
            .as_f64()
            .map(|v| v as f32)
            .ok_or_else(|| format!("water optical {key} required"))
    };
    let reflection = o["planar_reflection"]
        .as_bool()
        .ok_or("water reflection selection required")?;
    let mean = n("mean_height")? as f64;
    let mut ripples = Vec::new();
    for r in packet["ripples"]
        .as_array()
        .filter(|a| a.len() <= MAX_RIPPLES)
        .ok_or("bounded water ripples required")?
    {
        let r: [f32; 4] = serde_json::from_value(r.clone())
            .map_err(|_| "water ripple requires xy/age/strength")?;
        ripples.push(Ripple {
            uv: [r[0], r[1]],
            age: r[2],
            strength: r[3],
        });
    }
    let frame = DrawFrame {
        camera,
        vertices,
        indices,
        ripples,
        reflection_plane: reflection.then_some(ReflectionPlane {
            normal: up.map(f64::from),
            point: std::array::from_fn(|i| origin[i] + up[i] as f64 * mean),
        }),
        optics: Optics {
            time: n("time")?,
            murkiness: n("murkiness")?,
            light: vector3(&o["light"])?,
            sky_top: vector3(&o["sky_top"])?,
            sky_bottom: vector3(&o["sky_bottom"])?,
            sky_color_intensity: n("sky_color_intensity")?,
            neutral_reflection_intensity: n("neutral_reflection_intensity")?,
            planar_reflection: reflection,
            fullbright: o["fullbright"]==true,
            cave_fog: o["cave_fog"]==true,
            tangent_u,
            tangent_v,
            surface_up: up,
            ripple_origin: [0.; 2],
            micro_phase: vector3(&o["micro_phase"])?,
        },
    };
    frame.validate()?;
    Ok(frame)
}
/// BaseEntity can call this before any native resource is acquired. Its dummy
/// camera only checks attribute validity; actual backend matrices arrive later.
pub(crate) fn validate_numeric(packet:&crate::water::presentation::Packet)->Result<(),String>{
    if packet.rows.len()>MAX_VERTICES||packet.indices.len()>MAX_INDICES||packet.indices.len()%3!=0
        ||packet.indices.iter().any(|&i|i as usize>=packet.rows.len())
        ||packet.rows.iter().any(|r|{let r=r.map(|a|a.map(|x|x as f32));!Vertex{position:r[0],shading_normal:r[1],geometric_normal:r[2],axis_normal:r[3],uv_flow:r[4],depth_class_flags:r[5]}.valid()}) {
        return Err("invalid bounded numeric water mesh".into());
    }
    let identity=[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]];
    let camera=Camera{origin:packet.frame.origin,view:identity,projection:identity,relative_position:[0.;3],right:[1.,0.,0.],up:[0.,1.,0.],forward:[0.,0.,-1.],width:1,height:1,near:0.1,far:100.,reversed_z:false};
    draw_parts(&packet.metadata(),camera,std::iter::empty(),Vec::new()).map(|_|())
}
pub(crate) fn valid_packet(packet: &Value) -> bool {
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let origin = serde_json::from_value(packet["frame"]["origin"].clone()).unwrap_or([f64::NAN; 3]);
    draw_frame(
        packet,
        Camera {
            origin,
            view: identity,
            projection: identity,
            relative_position: [0.; 3],
            right: [1., 0., 0.],
            up: [0., 1., 0.],
            forward: [0., 0., -1.],
            width: 1,
            height: 1,
            near: 0.1,
            far: 100.,
            reversed_z: false,
        },
    )
    .is_ok()
}

#[cfg(test)] mod numeric_transport_tests {
 use super::*;
 #[test] fn numeric_water_matches_json_at_large_origins_and_rejects_corruption(){
  let frame=crate::water::math::Frame::radial([0.,6_371_000.,0.],[0.,0.,-1.]);
  let mesh=crate::water::surface_mesh::Mesh::default();
  let mut p=crate::water::presentation::Packet{frame,rows:std::sync::Arc::new(crate::water::presentation::mesh_rows(frame,&mesh)),indices:std::sync::Arc::new(vec![0,1,2]),
   optics:serde_json::json!({"time":3.25,"murkiness":0.,"sky_color_intensity":0.4,"neutral_reflection_intensity":0.5,"light":[0.4,0.8,-0.3],"sky_top":[0.1,0.2,0.3],"sky_bottom":[0.7,0.8,0.9],"planar_reflection":true,"mean_height":0.4,"micro_phase":[0.1,0.2,0.3]}),ripples:serde_json::json!([[0.5,0.75,0.2,1.]])};
  p.rows=std::sync::Arc::new((0..3).map(|i|[[i as f64*1.123456789,0.123456789,-7.,1.],[0.,1.,0.,0.],[0.,1.,0.,0.],[0.,1.,0.,0.],[0.25,0.5,0.1,0.2],[2.,3.,2.,0.]]).collect());
  validate_numeric(&p).unwrap();
  let identity=[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]];
  for delta in [[0.;3],[120.25,5.,-73.5],[-0.125,-17.5,0.25]]{
   let camera=Camera{origin:std::array::from_fn(|i|frame.origin[i]+delta[i]),view:identity,projection:identity,relative_position:[0.;3],right:[1.,0.,0.],up:[0.,1.,0.],forward:[0.,0.,-1.],width:1920,height:1200,near:0.05,far:1e7,reversed_z:false};
   let a=draw_frame(&p.json(),camera.clone()).unwrap();let b=draw_numeric(&p,camera).unwrap();assert_eq!(a.vertex_bytes(),b.vertex_bytes());assert_eq!(a.index_bytes(),b.index_bytes());assert_eq!(a.vertex_bytes(),a.vertex_bytes_borrowed());assert_eq!(b.vertex_bytes(),b.vertex_bytes_borrowed());assert_eq!(a.index_bytes(),a.index_bytes_borrowed());
   assert_eq!(format!("{:?}",a.optics),format!("{:?}",b.optics)); assert_eq!(format!("{:?}",a.reflection_plane),format!("{:?}",b.reflection_plane));
  }
  let original=p.clone();std::sync::Arc::make_mut(&mut p.indices)[0]=999;assert!(validate_numeric(&p).is_err());validate_numeric(&original).unwrap();
  let mut bad=original;std::sync::Arc::make_mut(&mut bad.rows)[0][0][0]=f64::NAN;assert!(validate_numeric(&bad).is_err());
 }
}

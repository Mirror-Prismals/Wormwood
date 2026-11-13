//! Presentation math for the initial closed convex mesh. No terrain or world storage.
use serde_json::Value;
pub fn valid(view: &Value) -> bool {
    (view["aspect"].is_null()
        || view["aspect"]
            .as_f64()
            .is_some_and(|x| x.is_finite() && (0.2..=8.).contains(&x)))
        && ["right", "up", "forward"].iter().all(|key| {
            view[*key].is_null()
                || view[*key].as_array().is_some_and(|v| {
                    v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
                })
        })
        && view["entity"].as_u64().is_some_and(|n| n > 0)
        && view["position"].as_array().is_some_and(|v| {
            v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
        })
        && ["yaw", "pitch"]
            .iter()
            .all(|key| view[*key].as_f64().is_some_and(f64::is_finite))
}
pub struct Projector {
    camera:[f64;3],right:[f64;3],up:[f64;3],forward:[f64;3],
    horizontal_focal:f64,focal:f64,depth_scale:f64,depth_offset:f64,two_sided:bool,
}
impl Projector {
    pub fn new(view:&Value)->Self {
        let camera=std::array::from_fn(|i|view["position"][i].as_f64().unwrap());
    let (sy, cy) = view["yaw"].as_f64().unwrap().sin_cos();
    let (sp, cp) = view["pitch"].as_f64().unwrap().sin_cos();
    let basis = |key: &str, fallback: [f64; 3]| {
        if view[key].is_array() {
            std::array::from_fn(|i| view[key][i].as_f64().unwrap())
        } else {
            fallback
        }
    };
    let right = basis("right", [cy, 0., sy]);
    let up = basis("up", [-sy * sp, cp, cy * sp]);
    let forward = basis("forward", [sy * cp, sp, -cy * cp]);
    let focal = 1. / (70f64.to_radians() / 2.).tan();
    let near = view["near"].as_f64().unwrap_or(0.05).max(0.01);
    let far = view["far"].as_f64().unwrap_or(1000.);
    let horizontal_focal = focal / view["aspect"].as_f64().unwrap_or(4. / 3.);
    let depth_scale = far / (far - near);
    let depth_offset = far * near / (far - near);
    let two_sided = view["two_sided"] == true;
        Self {camera,right,up,forward,horizontal_focal,focal,depth_scale,depth_offset,two_sided}
    }
    /// Conservative hierarchy rejection before per-face transformation. Match
    /// the existing relative-f32 input; allow rounding slack at every clip plane.
    pub fn bounds_visible(&self,min:[f64;3],max:[f64;3],eye:[f64;3])->bool {
        let mut outside=[true;6];
        for corner in 0..8 {
            let p:[f64;3]=std::array::from_fn(|k|((if corner&(1<<k)==0{min[k]}else{max[k]})-eye[k])as f32 as f64-self.camera[k]);
            let dot=|axis:[f64;3]|axis.iter().zip(p).map(|(a,b)|a*b).sum::<f64>();
            let w=dot(self.forward);let x=dot(self.right)*self.horizontal_focal;let y=dot(self.up)*self.focal;let z=self.depth_scale*w-self.depth_offset;
            let slack=1e-5*(1.+x.abs()+y.abs()+z.abs()+w.abs());
            for (i,d) in [x-w,-x-w,y-w,-y-w,z-w,-z].into_iter().enumerate(){outside[i]&=d>slack;}
        }
        !outside.into_iter().any(|v|v)
    }
    pub fn triangle(&self, triangle:&[[f32;6]])->Option<[f32;24]> {
        debug_assert_eq!(triangle.len(),3);
        let Self {camera,right,up,forward,horizontal_focal,focal,depth_scale,depth_offset,two_sided}=*self;
        if !two_sided {
            let mut a = [0f64; 3];
            let mut b = [0f64; 3];
            let mut toward = [0f64; 3];
            for i in 0..3 {
                a[i] = (triangle[1][i] - triangle[0][i]) as f64;
                b[i] = (triangle[2][i] - triangle[0][i]) as f64;
                toward[i] = camera[i] - triangle[0][i] as f64;
            }
            let normal = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            if normal.iter().zip(toward).map(|(n, t)| n * t).sum::<f64>() <= 0. {
                return None;
            }
        }
        let mut data = [0f32; 24];
        for (i, v) in triangle.iter().enumerate() {
            let relative = [
                v[0] as f64 - camera[0],
                v[1] as f64 - camera[1],
                v[2] as f64 - camera[2],
            ];
            let dot = |axis: [f64; 3]| axis.iter().zip(relative).map(|(a, b)| a * b).sum::<f64>();
            let z = dot(forward);
            data[i * 4..i * 4 + 4].copy_from_slice(&[
                (dot(right) * horizontal_focal) as f32,
                (dot(up) * focal) as f32,
                (depth_scale * z - depth_offset) as f32,
                z as f32,
            ]);
            data[12 + i * 4..16 + i * 4].copy_from_slice(&[v[3], v[4], v[5], 1.]);
        }
        Some(data)
    }
}
/// Conservative homogeneous clip rejection. Crossing/near-plane faces survive.
/// Uses WebGPU/OpenGL's shared pre-remap scene depth convention (0 <= z <= w).
pub fn in_frustum(t: &[f32;24]) -> bool {
    !(t[..12].chunks_exact(4).all(|p|p[3]<=0.)
        || (0..3).any(|axis| {
            t[..12].chunks_exact(4).all(|p|p[axis]>p[3])
            || t[..12].chunks_exact(4).all(|p|p[axis]<if axis==2 {0.}else{-p[3]})
        }))
}

pub fn project(vertices:&[[f32;6]],view:&Value)->Vec<[f32;24]> {
    let projector=Projector::new(view);
    let mut output=Vec::with_capacity(vertices.len()/3);
    output.extend(vertices.chunks_exact(3).filter_map(|tri|projector.triangle(tri)));
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn front_face_projects_and_reverse_winding_is_culled() {
        let tri = [
            [-1., -1., 0., 1., 0., 0.],
            [1., -1., 0., 1., 0., 0.],
            [0., 1., 0., 1., 0., 0.],
        ];
        let view = json!({"entity":1,"position":[0,0,4],"yaw":0,"pitch":0});
        let projected = project(&tri, &view);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0][3], 4.);
        assert!(projected[0][2] > 0. && projected[0][2] < 4.);
        assert!(project(&[tri[0], tri[2], tri[1]], &view).is_empty());
    }
}

#[cfg(test)]
mod scene_cpu_projection_tests {
use super::*;
fn reference_project(vertices: &[[f32; 6]], view: &Value) -> Vec<[f32; 24]> {
    let camera: Vec<f64> = view["position"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let (sy, cy) = view["yaw"].as_f64().unwrap().sin_cos();
    let (sp, cp) = view["pitch"].as_f64().unwrap().sin_cos();
    let basis = |key: &str, fallback: [f64; 3]| {
        if view[key].is_array() {
            std::array::from_fn(|i| view[key][i].as_f64().unwrap())
        } else {
            fallback
        }
    };
    let right = basis("right", [cy, 0., sy]);
    let up = basis("up", [-sy * sp, cp, cy * sp]);
    let forward = basis("forward", [sy * cp, sp, -cy * cp]);
    let focal = 1. / (70f64.to_radians() / 2.).tan();
    let near = view["near"].as_f64().unwrap_or(0.05).max(0.01);
    let far = view["far"].as_f64().unwrap_or(1000.);
    let horizontal_focal = focal / view["aspect"].as_f64().unwrap_or(4. / 3.);
    let depth_scale = far / (far - near);
    let depth_offset = far * near / (far - near);
    let two_sided = view["two_sided"] == true;
    let mut output = Vec::with_capacity(vertices.len() / 3);
    for triangle in vertices.chunks_exact(3) {
        if !two_sided {
            let mut a = [0f64; 3];
            let mut b = [0f64; 3];
            let mut toward = [0f64; 3];
            for i in 0..3 {
                a[i] = (triangle[1][i] - triangle[0][i]) as f64;
                b[i] = (triangle[2][i] - triangle[0][i]) as f64;
                toward[i] = camera[i] - triangle[0][i] as f64;
            }
            let normal = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            if normal.iter().zip(toward).map(|(n, t)| n * t).sum::<f64>() <= 0. {
                continue;
            }
        }
        let mut data = [0f32; 24];
        for (i, v) in triangle.iter().enumerate() {
            let relative = [
                v[0] as f64 - camera[0],
                v[1] as f64 - camera[1],
                v[2] as f64 - camera[2],
            ];
            let dot = |axis: [f64; 3]| axis.iter().zip(relative).map(|(a, b)| a * b).sum::<f64>();
            let z = dot(forward);
            data[i * 4..i * 4 + 4].copy_from_slice(&[
                (dot(right) * horizontal_focal) as f32,
                (dot(up) * focal) as f32,
                (depth_scale * z - depth_offset) as f32,
                z as f32,
            ]);
            data[12 + i * 4..16 + i * 4].copy_from_slice(&[v[3], v[4], v[5], 1.]);
        }
        output.push(data);
    }
    output
}

#[test]
fn scene_cpu_projector_preserves_projection_bits() {
    let mut vertices=Vec::new();
    for i in 0..90 {
        let x=(i%9) as f32*0.7-3.;let y=(i/9) as f32*0.4-2.;let z=(i%13) as f32*0.3-2.;
        vertices.extend([[x,y,z,0.2,0.4,0.8],[x+0.3,y,z-0.1,0.5,0.1,0.2],[x,y+0.6,z,0.9,0.8,0.1]]);
    }
    for two_sided in [false,true] {for yaw in [0.,0.8,3.14] {
        let view=serde_json::json!({"position":[0.25,-0.3,1.2],"yaw":yaw,"pitch":0.3,"aspect":1.6,"near":0.05,"far":12742000.,"two_sided":two_sided});
        let bits=|v:Vec<[f32;24]>|v.into_iter().map(|t|t.map(f32::to_bits)).collect::<Vec<_>>();
        assert_eq!(bits(project(&vertices,&view)),bits(reference_project(&vertices,&view)));
    }}
}
}

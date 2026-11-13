//! Camera and reflection data shared by native scene connections.
use super::water::{
    data::{Camera, DrawFrame},
    reflection,
};
use crate::vegetation::math::*;
use serde_json::Value;
/// Same native position/attribute interleave, with one capacity check and
/// contiguous append per triangle instead of six four-float appends.
pub(crate) fn interleave_batch(triangles:&[[f32;24]],out:&mut Vec<f32>){
    out.clear();out.reserve(triangles.len()*24);
    for t in triangles {
        out.extend_from_slice(&[t[0],t[1],t[2],t[3],t[12],t[13],t[14],t[15],
            t[4],t[5],t[6],t[7],t[16],t[17],t[18],t[19],
            t[8],t[9],t[10],t[11],t[20],t[21],t[22],t[23]]);
    }
}
#[cfg(test)]mod interleave_tests {
 use super::*;
 #[test]fn native_interleave_preserves_all_float_bits_and_reused_allocation(){
  let rows:Vec<[f32;24]>=(0..4096).map(|i|std::array::from_fn(|j|f32::from_bits((i*24+j)as u32*40321))).collect();
  let mut reference=Vec::new();for t in &rows{for v in 0..3{reference.extend_from_slice(&t[v*4..v*4+4]);reference.extend_from_slice(&t[12+v*4..16+v*4]);}}
  let mut out=Vec::new();interleave_batch(&rows,&mut out);assert!(out.iter().zip(&reference).all(|(a,b)|a.to_bits()==b.to_bits()));
  let pointer=out.as_ptr();interleave_batch(&rows[..100],&mut out);assert_eq!(pointer,out.as_ptr());assert_eq!(out.len(),2400);
  interleave_batch(&[],&mut out);assert!(out.is_empty());
 }
}
pub(crate) fn camera(view: &Value, width: u32, height: u32, gl: bool) -> Result<Camera, String> {
    let origin: V =
        serde_json::from_value(view["position"].clone()).map_err(|_| "scene position required")?;
    let (sy, cy) = view["yaw"].as_f64().ok_or("scene yaw required")?.sin_cos();
    let (sp, cp) = view["pitch"]
        .as_f64()
        .ok_or("scene pitch required")?
        .sin_cos();
    let axis = |key: &str, fallback: V| -> Result<V, String> {
        if view[key].is_null() {
            Ok(fallback)
        } else {
            serde_json::from_value(view[key].clone()).map_err(|_| "invalid scene basis".into())
        }
    };
    let right = axis("right", [cy, 0., sy])?;
    let up = axis("up", [-sy * sp, cp, cy * sp])?;
    let forward = axis("forward", [sy * cp, sp, -cy * cp])?;
    let near = 0.05_f64;
    let far = length(origin) + 2. * crate::terrain::RADIUS;
    let focal = 1. / 35f64.to_radians().tan();
    let aspect = view["aspect"]
        .as_f64()
        .unwrap_or(width as f64 / height as f64);
    let mut projection = [[0.; 4]; 4];
    projection[0][0] = (focal / aspect) as f32;
    projection[1][1] = focal as f32;
    projection[2][2] = (far / (far - near)) as f32;
    projection[2][3] = 1.;
    projection[3][2] = (-far * near / (far - near)) as f32;
    if gl {
        for column in &mut projection {
            column[2] = 2. * column[2] - column[3];
        }
    }
    let view = std::array::from_fn(|c| {
        if c < 3 {
            [right[c] as f32, up[c] as f32, forward[c] as f32, 0.]
        } else {
            [0., 0., 0., 1.]
        }
    });
    let camera = Camera {
        origin,
        view,
        projection,
        relative_position: [0.; 3],
        right: right.map(|v| v as f32),
        up: up.map(|v| v as f32),
        forward: forward.map(|v| v as f32),
        width,
        height,
        near: near as f32,
        far: far as f32,
        reversed_z: false,
    };
    if !camera.valid() {
        return Err("invalid scene camera".into());
    }
    Ok(camera)
}
pub(crate) fn water_frames(
    packets: &[crate::water::presentation::Packet],
    view: &Value,
    width: u32,
    height: u32,
    gl: bool,
) -> Result<Vec<DrawFrame>, String> {
    let camera = camera(view, width, height, gl)?;
    if packets.len()>8{return Err("bounded water packet array required".into());}
    packets.iter().map(|p|super::water::data::draw_numeric(p,camera.clone())).collect()
}
/// Reconstruct camera-relative positions from the shared homogeneous triangle ABI,
/// clip against the water plane, and project actual geometry under the reflected view.
pub(crate) fn reflected_batch(
    triangles: &[[f32; 24]],
    frame: &DrawFrame,
) -> Result<Vec<[f32; 24]>, String> {
    let plane = frame.reflection_plane.ok_or("reflection plane required")?;
    let reflected = reflection::scene_view(&frame.camera, plane)?;
    let camera = &frame.camera;
    let right = camera.right.map(f64::from);
    let up = camera.up.map(f64::from);
    let forward = camera.forward.map(f64::from);
    let clip = reflected.clip_plane.map(f64::from);
    let mut projection = camera.projection;
    if projection[3][2] < -camera.near * 1.5 {
        for column in &mut projection {
            column[2] = (column[2] + column[3]) * 0.5;
        }
    }
    let transform = |p: V| -> [f32; 4] {
        let v: [f32; 4] = std::array::from_fn(|r| {
            (0..3)
                .map(|c| reflected.camera.view[c][r] * p[c] as f32)
                .sum::<f32>()
                + reflected.camera.view[3][r]
        });
        std::array::from_fn(|r| (0..4).map(|c| projection[c][r] * v[c]).sum())
    };
    let mut out = Vec::with_capacity(triangles.len());
    for triangle in triangles {
        if triangle[15] <= -9.5 && triangle[15] > -12. {
            // Sky rays use the solar frame of the current floating origin.
            // Reflect directions through the actual water plane, with no finite
            // world-position reconstruction of the clip-space background quad.
            let radial = unit(camera.origin);
            let (east, north) = basis(radial);
            let n = [
                dot(plane.normal, east),
                dot(plane.normal, radial),
                dot(plane.normal, north),
            ];
            let mut sky = *triangle;
            for i in 0..3 {
                let ray = [
                    sky[12 + i * 4] as f64,
                    sky[13 + i * 4] as f64,
                    sky[14 + i * 4] as f64,
                ];
                let reflected = sub(ray, mul(n, 2. * dot(ray, n)));
                for k in 0..3 {
                    sky[12 + i * 4 + k] = reflected[k] as f32;
                }
            }
            out.push(sky);
            continue;
        }
        // Screen-space feedback has no world position or reflected appearance.
        if triangle[15] <= -4. && triangle[15] > -9.5 {
            continue;
        }
        let mut polygon = [([0.; 3], [0.; 4]); 3];
        for i in 0..3 {
            let t = &triangle[i * 4..i * 4 + 4];
            let p = add(
                add(
                    mul(right, t[0] as f64 / camera.projection[0][0] as f64),
                    mul(up, t[1] as f64 / camera.projection[1][1] as f64),
                ),
                mul(forward, t[3] as f64),
            );
            polygon[i] = (p, triangle[12 + i * 4..16 + i * 4].try_into().unwrap());
        }
        let distance = |p: V| dot([clip[0], clip[1], clip[2]], p) + clip[3];
        // One plane clips a triangle into at most four vertices. Stack storage
        // avoids two heap allocations for every reflected terrain triangle.
        let mut clipped = [([0.; 3], [0.; 4]); 4];
        let mut count = 0;
        for i in 0..polygon.len() {
            let (a, ca) = polygon[i];
            let (b, cb) = polygon[(i + 1) % polygon.len()];
            let da = distance(a);
            let db = distance(b);
            if da >= 0. {
                clipped[count] = (a, ca);
                count += 1;
            }
            if (da >= 0.) != (db >= 0.) {
                let t = da / (da - db);
                clipped[count] = (
                    add(a, mul(sub(b, a), t)),
                    std::array::from_fn(|k| ca[k] + (cb[k] - ca[k]) * t as f32),
                );
                count += 1;
            }
        }
        if count < 3 {
            continue;
        }
        for i in 1..count - 1 {
            let mut triangle = [0.; 24];
            for (j, (p, color)) in [clipped[0], clipped[i], clipped[i + 1]]
                .into_iter()
                .enumerate()
            {
                triangle[j * 4..j * 4 + 4].copy_from_slice(&transform(p));
                triangle[12 + j * 4..16 + j * 4].copy_from_slice(&color);
            }
            // Reject before upload; clipping can produce geometry outside the
            // reflected camera even when the source was visible in the main view.
            if visible_clip_triangle(&triangle) {
                out.push(triangle);
            }
        }
    }
    if out.len() > crate::scene_limits::MAX_REFLECTED_TRIANGLES {
        return Err("reflected scene triangle budget exceeded".into());
    }
    Ok(out)
}

fn visible_clip_triangle(t: &[f32; 24]) -> bool {
    if (0..3).all(|i| t[i * 4 + 3] <= 0.) {
        return false;
    }
    !(0..3).any(|k| {
        (0..3).all(|i| t[i * 4 + k] > t[i * 4 + 3])
            || (0..3).all(|i| t[i * 4 + k] < if k == 2 { 0. } else { -t[i * 4 + 3] })
    })
}

pub(crate) fn reflected_plants(
    packet: &crate::vegetation::packet::Packet,
    frame: &DrawFrame,
) -> Result<crate::vegetation::packet::Packet, String> {
    use crate::vegetation::packet::{Packet, Scene};
    let plane = frame
        .reflection_plane
        .ok_or("plant reflection plane required")?;
    let reflected = reflection::scene_view(&frame.camera, plane)?;
    let distance = |p: &[f32; 16]| {
        (0..3).map(|i| reflected.clip_plane[i] * p[i]).sum::<f32>() + reflected.clip_plane[3]
    };
    let mut vertices = Vec::with_capacity(packet.indices.len());
    let mut indices = Vec::with_capacity(packet.indices.len());
    for face in packet.indices.chunks_exact(3) {
        let points: [[f32; 16]; 3] = std::array::from_fn(|i| packet.vertices[face[i] as usize]);
        let mut polygon = [[0.; 16]; 4];
        let mut count = 0usize;
        for i in 0..3 {
            let a = points[i];
            let b = points[(i + 1) % 3];
            let da = distance(&a);
            let db = distance(&b);
            if da >= 0. {
                polygon[count] = a;
                count += 1;
            }
            if (da >= 0.) != (db >= 0.) {
                let t = da / (da - db);
                let mut vertex = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                vertex[11] = a[11];
                polygon[count] = vertex;
                count += 1;
            }
        }
        for i in 1..count.saturating_sub(1) {
            let first = vertices.len() as u32;
            vertices.extend([polygon[0], polygon[i], polygon[i + 1]]);
            indices.extend([first, first + 1, first + 2]);
        }
    }
    if vertices.len() > 98304 || indices.len() > 98304 {
        return Err("reflected plant triangle budget exceeded".into());
    }
    let mut projection = frame.camera.projection;
    if projection[3][2] < -frame.camera.near * 1.5 {
        for column in &mut projection {
            column[2] = (column[2] + column[3]) * 0.5;
        }
    }
    let view_projection = std::array::from_fn(|c| {
        std::array::from_fn(|r| {
            (0..4)
                .map(|k| projection[k][r] * reflected.camera.view[c][k])
                .sum()
        })
    });
    let scene = Scene {
        view_projection,
        sun_view_projection: packet.scene.sun_view_projection,
        sun_direction: packet.scene.sun_direction,
        sun_color: packet.scene.sun_color,
        ambient_color: packet.scene.ambient_color,
        options: packet.scene.options,
    };
    scene.validate()?;
    Ok(Packet {
        origin: packet.origin,
        revision: packet.revision,
        vertices,
        indices,
        scene,
    })
}

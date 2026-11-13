//! Phone lamp on ordinary scene triangles. The existing packet contains baked
//! RGB rather than original albedo, so the additive reflection is an estimate.
//! Coordinate-payload materials and separate vegetation are not relit here.
//! A fixed cone-depth grid gives conservative, coarse shadows, not a shadow map
//! with pixel accuracy; silhouettes smaller than its angular spacing can be missed.
use crate::audio::geometry::Geometry;
use serde_json::Value;

type V = [f64; 3];
const RANGE: f64 = 28.;
const INTENSITY: f64 = 2.0;
const MAX_EXTRA_TRIANGLES: usize = 16_384;
const SAMPLE_EDGE: f64 = 0.5;
const GRID: usize = 17;
const LIGHT_COLOR: [f64; 3] = [234. / 255., 242. / 255., 1.];

fn add(a: V, b: V) -> V { std::array::from_fn(|i| a[i] + b[i]) }
fn sub(a: V, b: V) -> V { std::array::from_fn(|i| a[i] - b[i]) }
fn mul(a: V, s: f64) -> V { a.map(|x| x * s) }
fn dot(a: V, b: V) -> f64 { a.iter().zip(b).map(|(a, b)| a * b).sum() }
fn cross(a: V, b: V) -> V {
    [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
}
fn length(a: V) -> f64 { dot(a, a).sqrt() }
fn unit(a: V) -> V { mul(a, 1. / length(a).max(1e-12)) }
fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x-a)/(b-a)).clamp(0., 1.);
    t*t*(3.-2.*t)
}
fn vector(v: &Value) -> Result<V, String> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or("flashlight requires a three-component vector")?;
    let mut out = [0.; 3];
    for i in 0..3 {
        out[i] = a[i].as_f64().filter(|n| n.is_finite()).ok_or("flashlight vector must be finite")?;
    }
    Ok(out)
}

struct Lamp {
    right: V,
    up: V,
    forward: V,
    local: V,
    world: V,
    focal: f64,
    horizontal_focal: f64,
    inner_cos: f64,
    outer_cos: f64,
    extent: f64,
}
impl Lamp {
    fn from_view(view: &Value) -> Result<Self, String> {
        let position = vector(&view["position"])?;
        let angle = |key: &str| -> Result<f64, String> {
            if view[key].is_null() { Ok(0.) }
            else { view[key].as_f64().filter(|v| v.is_finite()).ok_or_else(|| format!("flashlight {key} must be finite")) }
        };
        let (sy, cy) = angle("yaw")?.sin_cos();
        let (sp, cp) = angle("pitch")?.sin_cos();
        let basis = |key: &str, fallback: V| {
            if view[key].is_null() { Ok(fallback) } else { vector(&view[key]) }
        };
        let right = basis("right", [cy, 0., sy])?;
        let up = basis("up", [-sy*sp, cp, cy*sp])?;
        let forward = basis("forward", [sy*cp, sp, -cy*cp])?;
        if [right, up, forward].iter().any(|v| (length(*v)-1.).abs() > 1e-4)
            || [dot(right, up), dot(right, forward), dot(up, forward)].iter().any(|v| v.abs() > 1e-4)
        {
            return Err("flashlight camera basis must be orthonormal".into());
        }
        let aspect = if view["aspect"].is_null() { 4./3. } else {
            view["aspect"].as_f64().filter(|v| v.is_finite() && (0.2..=8.).contains(v))
                .ok_or("flashlight aspect is invalid")?
        };
        // Keep the optical axis on the aiming ray, including close surfaces.
        let local = mul(forward, 0.18);
        let world = add(position, local);
        if world.iter().any(|v| !v.is_finite()) { return Err("flashlight origin is invalid".into()); }
        let focal = 1. / (70f64.to_radians()/2.).tan();
        Ok(Self { right, up, forward, local, world, focal, horizontal_focal: focal/aspect,
            inner_cos: 22f64.to_radians().cos(), outer_cos: 42f64.to_radians().cos(),
            extent: 42f64.to_radians().tan() })
    }
    fn point(&self, row: &[f32; 24], vertex: usize) -> V {
        let p = vertex*4;
        add(add(mul(self.right, f64::from(row[p])/self.horizontal_focal),
            mul(self.up, f64::from(row[p+1])/self.focal)), mul(self.forward, f64::from(row[p+3])))
    }
    fn contribution(&self, point: V, normal: V) -> f64 {
        let offset = sub(point, self.local);
        let distance = length(offset);
        if distance <= 0.0001 || distance >= RANGE { return 0.; }
        let ray = mul(offset, 1./distance);
        let cosine = dot(ray, self.forward);
        let cone = smoothstep(self.outer_cos, self.inner_cos, cosine);
        if cone <= 0.0001 { return 0.; }
        let cardioid = (cosine*0.5+0.5).clamp(0., 1.).powi(2);
        let falloff = 1.-smoothstep(RANGE*0.52, RANGE, distance);
        let facing = 0.28+0.72*dot(normal, mul(ray, -1.)).max(0.);
        INTENSITY*cone*cardioid*falloff*facing
    }
}

#[derive(Clone, Copy)]
struct Blocker { point: V, normal: V, forward_depth: f64 }
struct Shadows { grid: [Option<Blocker>; GRID*GRID] }
impl Shadows {
    fn capture(lamp: &Lamp, geometry: Option<&dyn Geometry>) -> Self {
        let mut grid = [None; GRID*GRID];
        if let Some(geometry) = geometry {
            for y in 0..GRID {
                for x in 0..GRID {
                    let sx = (2.*x as f64/(GRID-1) as f64-1.)*lamp.extent;
                    let sy = (2.*y as f64/(GRID-1) as f64-1.)*lamp.extent;
                    let direction = unit(add(add(lamp.forward, mul(lamp.right, sx)), mul(lamp.up, sy)));
                    if let Some(hit) = geometry.trace(lamp.world, direction, RANGE) {
                        if !hit.distance.is_finite() || hit.distance < 0. || hit.distance > RANGE { continue; }
                        // Work in the light frame to retain centimetre precision
                        // when the world camera is Earth-radius coordinates.
                        let point = mul(direction, hit.distance);
                        let normal = if hit.normal.iter().all(|v| v.is_finite()) && length(hit.normal) > 1e-8 {
                            unit(hit.normal)
                        } else { [0.; 3] };
                        grid[y*GRID+x] = Some(Blocker { point, normal, forward_depth: dot(point, lamp.forward) });
                    }
                }
            }
        }
        Self { grid }
    }
    fn blocked(&self, lamp: &Lamp, point: V) -> bool {
        let relative = sub(point, lamp.local);
        let distance = length(relative);
        if distance < 1e-8 || distance >= RANGE { return false; }
        let depth = dot(relative, lamp.forward);
        if depth <= 0. || depth/distance < lamp.outer_cos { return false; }
        let coordinate = |axis: V| ((dot(relative, axis)/depth/lamp.extent+1.)*0.5*(GRID-1) as f64).clamp(0., (GRID-1) as f64);
        let x = (coordinate(lamp.right).floor() as usize).min(GRID-2);
        let y = (coordinate(lamp.up).floor() as usize).min(GRID-2);
        let ray = mul(relative, 1./distance);
        // The nearest of four neighboring blockers is conservative at edges.
        // Intersect each sampled surface plane with the receiver ray: simply
        // averaging radial distances would leak through walls and shadow the
        // illuminated face itself when it is slanted relative to the lamp.
        for index in [y*GRID+x, y*GRID+x+1, (y+1)*GRID+x, (y+1)*GRID+x+1] {
            let Some(hit) = self.grid[index] else { continue; };
            let denominator = dot(ray, hit.normal);
            let blocker_distance = if length(hit.normal) > 0.5 && denominator.abs() > 1e-8 {
                let t = dot(hit.point, hit.normal)/denominator;
                if t < 0. { continue; }
                t
            } else { hit.forward_depth/(depth/distance) };
            let bias = 0.002+blocker_distance.max(0.)*0.0005;
            if distance > blocker_distance+bias { return true; }
        }
        false
    }
}

/// Relight the ordinary scene packet before adding the phone overlay. Geometry
/// tracing is bounded to GRID² calls independent of scene triangle count.
/// Refinement adds at most MAX_EXTRA_TRIANGLES; off packets are unchanged.
pub fn illuminate(triangles: &mut Vec<[f32; 24]>, view: &Value, geometry: Option<&dyn Geometry>) -> Result<(), String> {
    if view["phone"]["equipped"] != true || view["phone"]["flashlight_on"] != true || triangles.is_empty() {
        return Ok(());
    }
    let lamp = Lamp::from_view(view)?;
    let shadows = Shadows::capture(&lamp, geometry);
    // Add samples only where the beam can intersect a face. Longest-edge
    // splitting preserves positions, depth and baked RGB by affine interpolation.
    // It finds illuminated interiors even when all original corners miss the cone.
    let originals = triangles.len();
    // Leave room for the phone/sky overlays within native scene limits.
    let mut extra = MAX_EXTRA_TRIANGLES.min(120_000usize.saturating_sub(originals));
    let mut pending = Vec::with_capacity(32);
    for index in 0..originals {
        let original = triangles[index];
        if !relightable(&original) { continue; }
        pending.push((original, 0u8));
        let mut first = true;
        while let Some((mut triangle, level)) = pending.pop() {
            let points = std::array::from_fn(|v| lamp.point(&triangle, v));
            let center = mul(add(add(points[0], points[1]), points[2]), 1./3.);
            let radius = points.iter().map(|p| length(sub(*p, center))).fold(0., f64::max);
            let offset = sub(center, lamp.local);
            let depth = dot(offset, lamp.forward);
            let radial = length(sub(offset, mul(lamp.forward, depth)));
            let intersects = length(offset)-radius < RANGE && depth+radius > 0.
                && radial <= depth.max(0.)*lamp.extent + radius*(1.+lamp.extent);
            let (edge, edge_length) = [(0,1),(1,2),(2,0)].into_iter()
                .map(|e| (e, length(sub(points[e.0], points[e.1]))))
                .max_by(|a,b| a.1.total_cmp(&b.1)).unwrap();
            if intersects && edge_length > SAMPLE_EDGE && level < 14 && extra > 0 {
                let (a,b) = split(&triangle, edge);
                extra -= 1;
                pending.push((b, level+1));
                pending.push((a, level+1));
                continue;
            }
            if intersects { shade(&mut triangle, &points, &lamp, &shadows); }
            if first { triangles[index] = triangle; first = false; }
            else { triangles.push(triangle); }
        }
    }
    Ok(())
}

// Positive material tags carry coordinates rather than RGB. Never change those
// payloads. Soil keeps RGB but packs two texture coordinates into its fourth lane.
fn relightable(t: &[f32; 24]) -> bool {
    t.iter().all(|v| v.is_finite()) && [15,19,23].iter().all(|i|
        (0.0..=1.0).contains(&t[*i]) || t[*i] >= 4096.)
}
fn split(t: &[f32; 24], (a,b): (usize,usize)) -> ([f32;24], [f32;24]) {
    let mut left = *t;
    let mut right = *t;
    for k in 0..4 {
        let position = (t[a*4+k]+t[b*4+k])*0.5;
        let data = if k == 3 && t[15] >= 4096. {
            let ca = t[15+a*4]-4096.; let cb = t[15+b*4]-4096.;
            // Decode before interpolation: averaging the packed scalar carries
            // fractional U into V and produces texture streaks.
            4096.+((ca%2048.+cb%2048.)*0.5).round()
                + (( (ca/2048.).floor()+(cb/2048.).floor())*0.5).round()*2048.
        } else { (t[12+a*4+k]+t[12+b*4+k])*0.5 };
        left[b*4+k] = position; right[a*4+k] = position;
        left[12+b*4+k] = data; right[12+a*4+k] = data;
    }
    (left,right)
}
fn shade(t: &mut [f32;24], points: &[V;3], lamp: &Lamp, shadows: &Shadows) {
    let normal = cross(sub(points[1],points[0]),sub(points[2],points[0]));
    if length(normal) < 1e-10 { return; }
    let normal = unit(normal);
    for vertex in 0..3 {
        let amount = lamp.contribution(points[vertex],normal);
        // Occlusion belongs to this sample, not every corner of the parent face.
        if amount <= 0. || shadows.blocked(lamp,points[vertex]) { continue; }
        let start = 12+vertex*4;
        let rgb: [f64;3] = std::array::from_fn(|i| f64::from(t[start+i]).max(0.));
        let largest = rgb.iter().copied().fold(0f64,f64::max);
        let reflectance = if largest > 1e-6 { rgb.map(|v| v/largest*largest.clamp(0.4,1.)) } else { [0.4;3] };
        for channel in 0..3 {
            t[start+channel] = (rgb[channel]+reflectance[channel]*LIGHT_COLOR[channel]*amount*0.8)
                .min(rgb[channel].max(1.)) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::geometry::{Hit, Snapshot};
    use serde_json::json;
    use std::cell::Cell;

    fn view() -> Value { json!({"position":[0,6371000,0],"right":[1,0,0],"up":[0,1,0],"forward":[0,0,-1],"yaw":0,"pitch":0,"aspect":1.5,"phone":{"equipped":true,"flashlight_on":true}}) }
    fn packet(lamp: &Lamp, distance: f64, color: f32) -> [f32; 24] {
        let center = add(lamp.local, mul(lamp.forward, distance));
        let points = [add(center, [-0.2,-0.2,0.]), add(center, [0.2,-0.2,0.]), add(center, [0.,0.2,0.])];
        let mut row = [0.; 24];
        for i in 0..3 {
            row[i*4] = (dot(points[i], lamp.right)*lamp.horizontal_focal) as f32;
            row[i*4+1] = (dot(points[i], lamp.up)*lamp.focal) as f32;
            row[i*4+2] = (dot(points[i], lamp.forward)-0.05) as f32;
            row[i*4+3] = dot(points[i], lamp.forward) as f32;
            row[12+i*4..16+i*4].copy_from_slice(&[color, color, color, 1.]);
        }
        row
    }
    #[test]
    fn brighter_wider_cone_keeps_smooth_falloff_and_facing() {
        let lamp = Lamp::from_view(&view()).unwrap();
        let normal = mul(lamp.forward, -1.);
        let point = |angle: f64, distance: f64| add(lamp.local, mul(add(mul(lamp.forward, angle.to_radians().cos()), mul(lamp.right, angle.to_radians().sin())), distance));
        assert!((lamp.contribution(point(0., 5.), normal)-INTENSITY).abs() < 1e-12);
        assert!(lamp.contribution(point(22., 5.), normal) > lamp.contribution(point(30., 5.), normal));
        assert_eq!(lamp.contribution(point(43., 5.), normal), 0.);
        assert!(lamp.contribution(point(0., 17.), normal) < lamp.contribution(point(0., 5.), normal));
        assert_eq!(lamp.contribution(point(0., RANGE), normal), 0.);
        assert!((lamp.contribution(point(0., 5.), lamp.forward)-INTENSITY*0.28).abs() < 1e-12);
    }
    #[test]
    fn dark_scene_lights_and_disabled_or_shader_packets_remain_exact() {
        let v = view(); let lamp = Lamp::from_view(&v).unwrap();
        let original = packet(&lamp, 5., 0.);
        let mut rows = vec![original];
        illuminate(&mut rows, &v, None).unwrap();
        assert!(rows[0][12] > 0.1 && rows[0][14] > rows[0][12]);
        assert_eq!(&rows[0][..12], &original[..12]);
        let mut disabled = v.clone(); disabled["phone"]["flashlight_on"] = json!(false);
        rows = vec![original]; illuminate(&mut rows, &disabled, None).unwrap(); assert_eq!(rows, [original]);
        disabled = v.clone(); disabled["phone"]["equipped"] = json!(false);
        illuminate(&mut rows, &disabled, None).unwrap(); assert_eq!(rows, [original]);
        let mut shader = original; shader[15] = -2.;
        rows = vec![shader]; illuminate(&mut rows, &v, None).unwrap(); assert_eq!(rows, [shader]);
    }
    #[test]
    fn near_wall_blocks_the_far_surface_but_its_front_is_lit() {
        let v = view(); let lamp = Lamp::from_view(&v).unwrap();
        let wall = Snapshot::parse(&json!({"revision":1,"origin":[0,6371000,0],"triangles":[],"solids":[{"min":[-10,-10,-2.3],"max":[10,10,-2.18]}]})).unwrap();
        let behind = packet(&lamp, 5., 0.02);
        let front = packet(&lamp, 2., 0.02);
        let mut rows = vec![behind, front];
        illuminate(&mut rows, &v, Some(&wall)).unwrap();
        assert_eq!(rows[0], behind);
        assert!(rows[1][12] > front[12]);
    }
    struct CountGeometry(Cell<usize>);
    impl Geometry for CountGeometry {
        fn revision(&self) -> u64 { 0 }
        fn trace(&self, _: V, _: V, _: f64) -> Option<Hit> { self.0.set(self.0.get()+1); None }
        fn wall_distance(&self, _: V, _: V, _: f64) -> f64 { panic!("flashlight must not request allocating volume queries") }
    }
    #[test]
    fn trace_budget_is_independent_of_triangle_count_and_off_costs_zero() {
        let mut v = view(); let lamp = Lamp::from_view(&v).unwrap();
        let mut rows = vec![packet(&lamp, 5., 0.01); 1024];
        let geometry = CountGeometry(Cell::new(0));
        illuminate(&mut rows, &v, Some(&geometry)).unwrap();
        assert_eq!(geometry.0.get(), GRID*GRID);
        v["phone"]["flashlight_on"] = json!(false);
        illuminate(&mut rows, &v, Some(&geometry)).unwrap();
        assert_eq!(geometry.0.get(), GRID*GRID);
    }
    #[test]
    fn reconstruction_preserves_rotated_camera_axes_and_rejects_invalid_view() {
        let mut v = view(); v["right"] = json!([0,0,1]); v["forward"] = json!([1,0,0]);
        let lamp = Lamp::from_view(&v).unwrap();
        let p = add(add(mul(lamp.right, 1.2), mul(lamp.up, -0.4)), mul(lamp.forward, 7.));
        let mut row = [0.; 24]; row[0] = (1.2*lamp.horizontal_focal) as f32; row[1] = (-0.4*lamp.focal) as f32; row[3] = 7.;
        assert!(length(sub(lamp.point(&row, 0), p)) < 1e-6);
        v["up"] = json!([0,0,0]);
        assert!(illuminate(&mut vec![row], &v, None).is_err());
    }
    fn broad_packet(lamp: &Lamp) -> [f32;24] {
        let mut row = packet(lamp,6.,0.02);
        for (i,(x,y)) in [(-12.,-8.),(12.,-8.),(0.,14.)].into_iter().enumerate() {
            row[i*4] = (x*lamp.horizontal_focal) as f32;
            row[i*4+1] = (y*lamp.focal) as f32;
        }
        row
    }
    #[test]
    fn beam_lights_face_interior_when_all_original_corners_miss() {
        let v=view(); let lamp=Lamp::from_view(&v).unwrap();
        let original=broad_packet(&lamp);
        for i in 0..3 { assert_eq!(lamp.contribution(lamp.point(&original,i),[0.,0.,1.]),0.); }
        let mut rows=vec![original]; illuminate(&mut rows,&v,None).unwrap();
        assert!(rows.len()>1 && rows.len()<=1+MAX_EXTRA_TRIANGLES);
        let mut lit_center=false; let mut dark_edge=false;
        for row in &rows { for i in 0..3 {
            let p=lamp.point(row,i); let radial=(p[0]*p[0]+p[1]*p[1]).sqrt();
            if radial<0.3 { assert!(row[12+i*4]>0.55); lit_center=true; }
            if radial>8. { assert_eq!(row[12+i*4],0.02); dark_edge=true; }
        }}
        assert!(lit_center && dark_edge);
    }
    #[test]
    fn aim_axis_is_centered_at_close_range_and_under_rotated_camera() {
        for (right,up,forward) in [([1.,0.,0.],[0.,1.,0.],[0.,0.,-1.]),
            ([0.,0.,1.],[-0.6,0.8,0.],[0.8,0.6,0.])] {
            let mut v=view(); v["right"]=json!(right); v["up"]=json!(up); v["forward"]=json!(forward);
            let lamp=Lamp::from_view(&v).unwrap();
            for distance in [0.5,2.,8.,20.] {
                let p=mul(forward,distance);
                assert!(dot(unit(sub(p,lamp.local)),forward)>1.-1e-10);
                assert!(lamp.contribution(p,mul(forward,-1.))>0.);
            }
        }
    }
    #[test]
    fn one_shadowed_corner_does_not_extinguish_the_lit_side() {
        let v=view(); let lamp=Lamp::from_view(&v).unwrap();
        let wall=Snapshot::parse(&json!({"revision":1,"origin":[0,6371000,0],"triangles":[],
            "solids":[{"min":[-10,-10,-2.3],"max":[-0.8,10,-2.18]}]})).unwrap();
        let mut rows=vec![broad_packet(&lamp)]; illuminate(&mut rows,&v,Some(&wall)).unwrap();
        let mut lit=false;let mut shadow=false;
        for row in rows {for i in 0..3 {
            let p=lamp.point(&row,i);
            if p[0]>1. && p[0]<2. && p[1].abs()<0.3 { lit |= row[12+i*4]>0.3; }
            if p[0] < -3. && p[0]>-4. && p[1].abs()<0.3 { shadow |= row[12+i*4]==0.02; }
        }}
        assert!(lit && shadow);
    }
    #[test]
    fn material_coordinates_are_never_reinterpreted_as_colors() {
        let v=view(); let lamp=Lamp::from_view(&v).unwrap();
        for tag in [-4096.,-2.,90.,100.,105.,106.,112.] {
            let mut t=broad_packet(&lamp);for i in [15,19,23] {t[i]=tag;}
            let mut rows=vec![t]; illuminate(&mut rows,&v,None).unwrap();assert_eq!(rows,vec![t]);
        }
        let mut t=broad_packet(&lamp);
        t[15]=4096.+10.+20.*2048.; t[19]=4096.+30.+22.*2048.; t[23]=4096.+20.+30.*2048.;
        let (a,b)=split(&t,(0,1));
        assert_eq!(a[19],4096.+20.+21.*2048.);assert_eq!(b[15],a[19]);
        let mut rows=vec![t];illuminate(&mut rows,&v,None).unwrap();
        assert!(rows.iter().any(|r|r[12]>0.3));
        assert!(rows.iter().all(|r|[15,19,23].iter().all(|i|r[*i]>=4096.)));
    }
    #[test]
    fn subdivision_budget_is_global_and_disabled_large_faces_are_exact() {
        let mut v=view();let lamp=Lamp::from_view(&v).unwrap();
        let original=broad_packet(&lamp);let mut rows=vec![original;64];
        illuminate(&mut rows,&v,None).unwrap();assert!(rows.len()<=64+MAX_EXTRA_TRIANGLES);
        v["phone"]["flashlight_on"]=json!(false);rows=vec![original;64];
        illuminate(&mut rows,&v,None).unwrap();assert_eq!(rows,vec![original;64]);
    }

}

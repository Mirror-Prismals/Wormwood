//! Immutable geometry boundary. Terrain supplies this trait through its own contracted owner.
use super::*;
use serde_json::Value;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub position: V,
    pub normal: V,
    pub distance: f64,
}
pub(crate) trait Geometry {
    fn revision(&self) -> u64;
    fn trace(&self, origin: V, direction: V, max_distance: f64) -> Option<Hit>;
    fn wall_distance(&self, start: V, end: V, max_distance: f64) -> f64;
    fn visible(&self, start: V, end: V, max_distance: f64) -> bool {
        let delta = sub(end, start);
        let n = length(delta);
        n < 1e-6
            || self
                .trace(start, unit(delta), n.min(max_distance))
                .is_none()
    }
}
#[derive(Default)]
pub(crate) struct Snapshot {
    revision: u64,
    origin: V,
    triangles: Vec<[V; 3]>,
    solids: Vec<[V; 2]>,
    oriented: Vec<OrientedSolid>,
}
struct OrientedSolid { center:V,half:V,yaw:f64 }
fn yaw(p:V,angle:f64)->V {let(s,c)=angle.sin_cos();[c*p[0]+s*p[2],p[1],-s*p[0]+c*p[2]]}
impl OrientedSolid {
    fn interval(&self,o:V,d:V,limit:f64)->Option<(f64,f64,V)> {
        interval(yaw(sub(o,self.center),-self.yaw),yaw(d,-self.yaw),[self.half.map(|n|-n),self.half],limit)
            .map(|(a,b,n)|(a,b,yaw(n,self.yaw)))
    }
}
impl Snapshot {
    /// Intervals through explicitly authored solid prisms, not mesh bounds.
    pub(crate) fn solid_intervals(&self,start:V,end:V)->Vec<[f64;2]> {
        let d=unit(sub(end,start));let limit=length(sub(end,start));let o=sub(start,self.origin);
        self.solids.iter().filter_map(|b| interval(o,d,*b,limit)).chain(self.oriented.iter().filter_map(|b|b.interval(o,d,limit)))
            .filter_map(|(a,z,_)|{let a=a.max(0.);let z=z.min(limit);(z>a).then_some([a,z])}).collect()
    }
    pub fn parse(v: &Value) -> Result<Self, String> {
        let revision = v["revision"]
            .as_u64()
            .ok_or("audio geometry revision required")?;
        let origin = vector(&v["origin"])?;
        let ts = v["triangles"]
            .as_array()
            .ok_or("audio triangles required")?;
        let bs = v["solids"]
            .as_array()
            .ok_or("audio solid bounds required")?;
        // Preserve both surfaces of the long enclosed flumes and the mine stairs.
        // This remains an immutable control/worker-side allocation.
        if ts.len() > 65536 || bs.len() > 4096 {
            return Err("audio geometry budget exceeded".into());
        }
        let mut triangles = Vec::with_capacity(ts.len());
        for t in ts {
            let a = t.as_array().ok_or("triangle array required")?;
            if a.len() != 3 {
                return Err("triangle needs three points".into());
            }
            let p = [vector(&a[0])?, vector(&a[1])?, vector(&a[2])?];
            if length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) < 1e-10 {
                return Err("degenerate audio triangle".into());
            }
            triangles.push(p);
        }
        let mut solids = Vec::with_capacity(bs.len());
        for b in bs {
            let min = vector(&b["min"])?;
            let max = vector(&b["max"])?;
            if (0..3).any(|i| min[i] >= max[i]) {
                return Err("invalid acoustic solid".into());
            }
            solids.push([min, max]);
        }
        let mut oriented=Vec::new();
        if let Some(rows)=v["oriented_solids"].as_array() {
            if rows.len()+solids.len()>4096 {return Err("oriented solid budget exceeded".into());}
            for row in rows {
                let center=vector(&row["center"])?;let size=vector(&row["size"])?;
                let yaw=row["yaw_degrees"].as_f64().filter(|n|n.is_finite()).ok_or("solid yaw required")?.to_radians();
                if size.iter().any(|n|*n<=0.){return Err("positive solid dimensions required".into());}
                oriented.push(OrientedSolid{center,half:mul(size,0.5),yaw});
            }
        }
        Ok(Self {
            revision,
            origin,
            triangles,
            solids,
            oriented,
        })
    }
}
fn interval(o: V, d: V, b: [V; 2], limit: f64) -> Option<(f64, f64, V)> {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut normal = [0.; 3];
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i] < b[0][i] || o[i] > b[1][i] {
                return None;
            }
            continue;
        }
        let mut a = (b[0][i] - o[i]) / d[i];
        let mut z = (b[1][i] - o[i]) / d[i];
        let mut n = -1.;
        if a > z {
            std::mem::swap(&mut a, &mut z);
            n = 1.;
        }
        if a > lo {
            lo = a;
            normal = [0.; 3];
            normal[i] = n;
        }
        hi = hi.min(z);
        if lo > hi {
            return None;
        }
    }
    if hi <= 1e-5 || lo >= limit {
        None
    } else {
        Some((lo, hi, normal))
    }
}
impl Geometry for Snapshot {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn trace(&self, origin: V, direction: V, max_distance: f64) -> Option<Hit> {
        let o = sub(origin, self.origin);
        let d = unit(direction);
        if length(d) < 0.5 {
            return None;
        }
        let mut nearest = max_distance;
        let mut normal = [0.; 3];
        let mut found = false;
        for b in &self.solids {
            if let Some((lo, hi, n)) = interval(o, d, *b, nearest) {
                let (t, n) = if lo > 1e-5 {
                    (lo, n)
                } else {
                    (hi, mul(d, -1.))
                };
                if t > 1e-5 && t < nearest {
                    nearest = t;
                    normal = n;
                    found = true;
                }
            }
        }
        for b in &self.oriented {
            if let Some((lo,hi,n))=b.interval(o,d,nearest) {
                let(t,n)=if lo>1e-5{(lo,n)}else{(hi,mul(d,-1.))};
                if t>1e-5 && t<nearest{nearest=t;normal=n;found=true;}
            }
        }
        for p in &self.triangles {
            let e1 = sub(p[1], p[0]);
            let e2 = sub(p[2], p[0]);
            let h = cross(d, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-10 {
                continue;
            }
            let inv = 1. / det;
            let s = sub(o, p[0]);
            let u = dot(s, h) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(d, q) * inv;
            if v < 0. || u + v > 1. {
                continue;
            }
            let t = dot(e2, q) * inv;
            if t > 1e-5 && t < nearest {
                nearest = t;
                normal = unit(cross(e1, e2));
                if dot(normal, d) > 0. {
                    normal = mul(normal, -1.)
                }
                found = true;
            }
        }
        found.then(|| Hit {
            position: add(origin, mul(d, nearest)),
            normal,
            distance: nearest,
        })
    }
    fn wall_distance(&self, start: V, end: V, max_distance: f64) -> f64 {
        // Bounds are volumetric acoustic solids. Surface triangles alone carry no invented thickness.
        let d = unit(sub(end, start));
        let limit = length(sub(end, start)).min(max_distance);
        let mut spans: Vec<(f64, f64)> = self.solid_intervals(start,add(start,mul(d,limit))).into_iter().map(|[a,b]|(a,b)).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut total, mut end) = (0., 0.);
        for (a, z) in spans {
            if z > end {
                total += z - a.max(end);
                end = z;
            }
        }
        total.max(0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oriented_authored_wall_has_actual_thickness() {
        let s=Snapshot::parse(&serde_json::json!({"revision":1,"origin":[0,6371000,0],"triangles":[],"solids":[],"oriented_solids":[{"center":[0,0,0],"size":[1,2,4],"yaw_degrees":45}]})).unwrap();
        let start=[-5.,6371000.,0.];let end=[5.,6371000.,0.];
        assert!((s.wall_distance(start,end,10.)-2_f64.sqrt()).abs()<1e-8);
        let h=s.trace(start,[1.,0.,0.],10.).unwrap();
        assert!((h.distance-(5.-0.5*2_f64.sqrt())).abs()<1e-8);
    }
    #[test]
    fn overlapping_solids_do_not_double_count_wall_thickness() {
        let s=Snapshot::parse(&serde_json::json!({"revision":7,"origin":[0,0,0],"triangles":[],"solids":[{"min":[1,-1,-1],"max":[3,1,1]},{"min":[2,-1,-1],"max":[4,1,1]}]})).unwrap();
        assert!((s.wall_distance([0.; 3], [6., 0., 0.], 64.) - 3.).abs() < 1e-10);
        let hit = s.trace([0.; 3], [1., 0., 0.], 64.).unwrap();
        assert!((hit.distance - 1.).abs() < 1e-10);
        assert_eq!(hit.normal, [-1., 0., 0.]);
        assert!(!s.visible([0.; 3], [6., 0., 0.], 64.));
    }
}

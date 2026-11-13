//! Finite convex material occupancy, independent of broadphase bounds.
use super::*;
use serde_json::Value;
#[derive(Clone)]
struct Plane {
    normal: V,
    offset: f64,
}
#[derive(Default)]
pub(crate) struct Volumes {
    origin: V,
    cells: Vec<Vec<Plane>>,
}
impl Volumes {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let origin = vector(&value["origin"])?;
        let mut cells = Vec::new();
        if let Some(rows) = value["tetrahedra"].as_array() {
            if rows.len() > 4096 {
                return Err("material tetrahedron budget exceeded".into());
            }
            for row in rows {
                let p: [V; 4] = serde_json::from_value(row.clone())
                    .map_err(|_| "material tetrahedron requires four points")?;
                if p.iter().flatten().any(|n| !n.is_finite())
                    || dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))).abs() < 1e-15
                {
                    return Err("invalid material tetrahedron".into());
                }
                let mut planes = Vec::new();
                for [a, b, c, d] in [[0, 1, 2, 3], [0, 1, 3, 2], [0, 2, 3, 1], [1, 2, 3, 0]] {
                    let mut n = unit(cross(sub(p[b], p[a]), sub(p[c], p[a])));
                    if dot(n, sub(p[d], p[a])) > 0. {
                        n = mul(n, -1.);
                    }
                    planes.push(Plane {
                        normal: n,
                        offset: dot(n, p[a]),
                    });
                }
                cells.push(planes);
            }
        }
        if let Some(rows) = value["wood"].as_array() {
            // Same complete-tree bound as VegetationSystem.QueryWood. This is
            // control/worker-side geometry preparation, never callback work.
            if rows.len() > crate::vegetation::wood_segment_limit() {
                return Err("wood geometry budget exceeded".into());
            }
            for row in rows {
                let a = vector(&row["start"])?;
                let b = vector(&row["end"])?;
                let radius = row["radius"]
                    .as_f64()
                    .filter(|r| r.is_finite() && *r > 0. && *r <= 10.)
                    .ok_or("invalid wood radius")?;
                let tip = row["end_radius"]
                    .as_f64()
                    .filter(|r| r.is_finite() && *r > 0. && *r <= 10.)
                    .ok_or("invalid wood tip radius")?;
                let axis = sub(b, a);
                let len = length(axis);
                if len < 1e-6 {
                    return Err("invalid wood segment".into());
                }
                let axis = unit(axis);
                let e = unit(cross(
                    if axis[1].abs() > 0.95 {
                        [0., 0., -1.]
                    } else {
                        [0., 1., 0.]
                    },
                    axis,
                ));
                let n = cross(axis, e);
                let mut planes = vec![
                    Plane {
                        normal: mul(axis, -1.),
                        offset: -dot(axis, a),
                    },
                    Plane {
                        normal: axis,
                        offset: dot(axis, b),
                    },
                ];
                // Seven-sided closed tapered wood, matching the near bark mesh.
                let apothem = (std::f64::consts::PI / 7.).cos();
                for side in 0..7 {
                    let angle = (side as f64 + 0.5) * std::f64::consts::TAU / 7.;
                    let radial = add(mul(e, angle.cos()), mul(n, angle.sin()));
                    let normal = sub(radial, mul(axis, (tip - radius) * apothem / len));
                    planes.push(Plane {
                        normal,
                        offset: dot(normal, a) + radius * apothem,
                    });
                }
                cells.push(planes);
            }
        }
        for planes in &mut cells {
            for plane in planes {
                let size = length(plane.normal);
                if size < 1e-12 {
                    return Err("degenerate material plane".into());
                }
                plane.normal = mul(plane.normal, 1. / size);
                plane.offset /= size;
            }
        }
        Ok(Self { origin, cells })
    }
    pub fn intervals(&self, start: V, direction: V, limit: f64) -> Vec<[f64; 2]> {
        let start = sub(start, self.origin);
        self.cells
            .iter()
            .filter_map(|planes| {
                let (mut lo, mut hi) = (0.0_f64, limit);
                for plane in planes {
                    let denominator = dot(plane.normal, direction);
                    let remaining = plane.offset - dot(plane.normal, start);
                    if denominator.abs() < 1e-12 {
                        if remaining < 0. {
                            return None;
                        }
                    } else if denominator > 0. {
                        hi = hi.min(remaining / denominator);
                    } else {
                        lo = lo.max(remaining / denominator);
                    }
                    if hi <= lo {
                        return None;
                    }
                }
                Some([lo, hi])
            })
            .collect()
    }
    pub fn trace(&self, start: V, direction: V, limit: f64) -> Option<geometry::Hit> {
        let local = sub(start, self.origin);
        let mut spans = Vec::new();
        for planes in &self.cells {
            let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
            let (mut enter, mut exit) = ([0.; 3], [0.; 3]);
            let mut missed = false;
            for plane in planes {
                let slope = dot(plane.normal, direction);
                let remaining = plane.offset - dot(plane.normal, local);
                if slope.abs() < 1e-12 {
                    if remaining < 0. {
                        missed = true;
                        break;
                    }
                } else {
                    let distance = remaining / slope;
                    if slope > 0. {
                        if distance < hi {
                            hi = distance;
                            exit = plane.normal;
                        }
                    } else if distance > lo {
                        lo = distance;
                        enter = plane.normal;
                    }
                    if hi <= lo {
                        missed = true;
                        break;
                    }
                }
            }
            if !missed && hi > 0. && lo < limit {
                spans.push((lo, hi, enter, exit));
            }
        }
        spans.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64, V, V)> = Vec::new();
        for span in spans {
            if let Some(last) = merged.last_mut().filter(|last| span.0 <= last.1 + 1e-9) {
                if span.1 > last.1 {
                    last.1 = span.1;
                    last.3 = span.3;
                }
            } else {
                merged.push(span);
            }
        }
        // Internal cell interfaces are not reflecting surfaces. Only boundaries
        // of the union of occupied material may reflect an acoustic ray.
        for (lo, hi, enter, exit) in merged {
            let (distance, normal) = if lo > 1e-5 { (lo, enter) } else { (hi, exit) };
            if distance > 1e-5 && distance < limit {
                return Some(geometry::Hit {
                    distance,
                    position: add(start, mul(direction, distance)),
                    normal,
                });
            }
        }
        None
    }
}
pub(crate) fn union(mut spans: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut result: Vec<[f64; 2]> = Vec::new();
    for span in spans {
        if let Some(last) = result.last_mut() {
            if span[0] <= last[1] {
                last[1] = last[1].max(span[1]);
                continue;
            }
        }
        result.push(span);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn authored_conifer_wood_bound_accepts_complete_query_and_rejects_overflow() {
        let count=crate::vegetation::wood_segment_limit();assert!(count>4096);
        let branch=json!({"start":[0.,0.,0.],"end":[0.,2.,0.],"radius":0.25,"end_radius":0.1});
        let mut packet=json!({"origin":[0.,0.,0.],"wood":vec![branch.clone();count]});
        let volumes=Volumes::parse(&packet).unwrap();assert_eq!(volumes.cells.len(),count);
        let spans=union(volumes.intervals([-1.,1.,0.],[1.,0.,0.],2.));assert_eq!(spans.len(),1);assert!(spans[0][1]>spans[0][0]);
        packet["wood"].as_array_mut().unwrap().push(branch);
        assert!(matches!(Volumes::parse(&packet),Err(e) if e=="wood geometry budget exceeded"));
        for profile in crate::vegetation::profiles::PROFILES {
            let branches=crate::vegetation::species::asset(profile).map_or(31,|a|a.branches.len());
            assert!(branches*crate::vegetation::WOOD_SITE_LIMIT<=count);
        }
    }
    #[test]
    fn tetrahedral_occupancy_does_not_fill_its_bounding_box() {
        let v = Volumes::parse(
            &json!({"origin":[0,0,0],"tetrahedra":[[[0,0,0],[1,0,0],[0,1,0],[0,0,1]]]}),
        )
        .unwrap();
        assert!(v.intervals([-1., 0.8, 0.8], [1., 0., 0.], 3.).is_empty());
        let spans = v.intervals([-1., 0.2, 0.2], [1., 0., 0.], 3.);
        assert!((spans[0][1] - spans[0][0] - 0.6).abs() < 1e-9);
        assert_eq!(
            union(vec![[1., 2.], [1.5, 3.], [4., 5.]]),
            vec![[1., 3.], [4., 5.]]
        );
    }
}

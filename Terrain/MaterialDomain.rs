//! Ownership boundaries between implicit resting matter and refined matter.
//! A boundary changes representation, never counts as excavated volume.
use serde_json::Value;
type V = [f64; 3];
pub(crate) const CELL_SIDE: f64 = 0.65;
#[derive(Clone)]
pub(crate) struct Domain {
    pub mined: bool,
    pub tetra: Option<[V; 4]>,
    pub origin: V,
    pub axes: [V; 3],
    pub min: V,
    pub max: V,
    pub slope: [f64; 2],
}
impl Domain {
    pub fn parse(e: &Value) -> Option<Self> {
        let d = &e["domain"];
        let result = Self {
            mined: e["mined"] == true,
            tetra: if d["tetra"].is_null() {
                None
            } else {
                Some(serde_json::from_value(d["tetra"].clone()).ok()?)
            },
            origin: serde_json::from_value(d["origin"].clone()).ok()?,
            axes: serde_json::from_value(d["axes"].clone()).ok()?,
            min: serde_json::from_value(d["min"].clone()).ok()?,
            max: serde_json::from_value(d["max"].clone()).ok()?,
            slope: if d["slope"].is_null() {
                [0.; 2]
            } else {
                serde_json::from_value(d["slope"].clone()).ok()?
            },
        };
        if result
            .origin
            .iter()
            .chain(result.axes.iter().flatten())
            .chain(result.min.iter())
            .chain(result.max.iter())
            .any(|v| !v.is_finite())
        {
            return None;
        }
        if result.slope.iter().any(|s| !s.is_finite() || s.abs() > 2.) {
            return None;
        }
        for i in 0..3 {
            if result.min[i] >= result.max[i] || result.max[i] - result.min[i] > 40. {
                return None;
            }
            for j in 0..3 {
                let dot: f64 = (0..3).map(|k| result.axes[i][k] * result.axes[j][k]).sum();
                if (dot - if i == j { 1. } else { 0. }).abs() > 1e-5 {
                    return None;
                }
            }
        }
        if let Some(vertices) = result.tetra {
            if !result.mined
                || vertices.iter().flatten().any(|x| !x.is_finite())
                || vertices
                    .iter()
                    .any(|p| (0..3).any(|k| p[k] < result.min[k] || p[k] > result.max[k]))
                || result
                    .planes()
                    .iter()
                    .any(|p| p.iter().any(|x| !x.is_finite()))
                || result.planes().iter().any(|p| {
                    vertices
                        .iter()
                        .map(|v| p[3] + (0..3).map(|k| p[k] * v[k]).sum::<f64>())
                        .fold(0., f64::max)
                        < 1e-6
                })
            {
                return None;
            }
        }
        Some(result)
    }
    pub fn local(&self, p: V) -> V {
        let mut q = self
            .axes
            .map(|a| (0..3).map(|k| a[k] * (p[k] - self.origin[k])).sum());
        q[1] -= self.slope[0] * q[0] + self.slope[1] * q[2];
        q
    }
    pub fn world(&self, mut p: V) -> V {
        p[1] += self.slope[0] * p[0] + self.slope[1] * p[2];
        std::array::from_fn(|k| {
            self.origin[k] + (0..3).map(|j| self.axes[j][k] * p[j]).sum::<f64>()
        })
    }
    // All half spaces are positive inside; tetrahedra and legacy boxes share
    // exactly the same clipping and collision definition.
    fn planes(&self) -> [[f64; 4]; 6] {
        if let Some(v) = self.tetra {
            let faces =
                [[0, 1, 2, 3], [0, 3, 1, 2], [1, 3, 2, 0], [2, 3, 0, 1]].map(|[a, b, c, other]| {
                    let u: V = std::array::from_fn(|k| v[b][k] - v[a][k]);
                    let w: V = std::array::from_fn(|k| v[c][k] - v[a][k]);
                    let mut n = [
                        u[1] * w[2] - u[2] * w[1],
                        u[2] * w[0] - u[0] * w[2],
                        u[0] * w[1] - u[1] * w[0],
                    ];
                    let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
                    let sign = if (0..3).map(|k| n[k] * (v[other][k] - v[a][k])).sum::<f64>() < 0. {
                        -1.
                    } else {
                        1.
                    };
                    n = n.map(|x| x * sign / len);
                    [
                        n[0],
                        n[1],
                        n[2],
                        -(0..3).map(|k| n[k] * v[a][k]).sum::<f64>(),
                    ]
                });
            return [faces[0], faces[1], faces[2], faces[3], faces[3], faces[3]];
        }
        std::array::from_fn(|i| {
            let k = i / 2;
            let upper = i % 2 == 1;
            let mut p = [0.; 4];
            p[k] = if upper { -1. } else { 1. };
            p[3] = if upper { self.max[k] } else { -self.min[k] };
            p
        })
    }
    pub fn distance(&self, p: V) -> f64 {
        let p = self.local(p);
        if self.tetra.is_none() {
            return (0..3)
                .map(|k| (self.min[k] - p[k]).max(p[k] - self.max[k]))
                .fold(f64::NEG_INFINITY, f64::max);
        }
        self.planes()
            .iter()
            .map(|plane| -plane[3] - (0..3).map(|k| plane[k] * p[k]).sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max)
    }
}
pub(crate) fn carve(p: V, d: f64, domains: &[Domain]) -> f64 {
    domains.iter().fold(d, |d, b| d.max(-b.distance(p)))
}
pub(crate) fn carve_bedrock(p: V, rest: f64, domains: &[Domain]) -> f64 {
    let floor =
        p.iter().map(|v| v * v).sum::<f64>().sqrt() - super::RADIUS - super::BEDROCK_ELEVATION;
    let earth = carve(p, rest, domains);
    if earth <= floor {
        return earth;
    }
    let bedrock = domains
        .iter()
        .filter(|d| d.mined)
        .fold(floor, |s, d| s.max(-d.distance(p)));
    earth.min(bedrock)
}
pub(crate) fn from_edits(edits: &[Value]) -> Vec<Domain> {
    edits.iter().filter_map(Domain::parse).collect()
}
pub(crate) fn clip(
    mut triangles: Vec<super::tool_cut::Triangle>,
    domains: &[Domain],
) -> Vec<super::tool_cut::Triangle> {
    for b in domains {
        let mut output = Vec::new();
        for tri in triangles {
            let local = tri.map(|p| b.local([p[0], p[1], p[2]]));
            if (0..3).any(|axis| {
                local.iter().all(|p| p[axis] < b.min[axis])
                    || local.iter().all(|p| p[axis] > b.max[axis])
            }) {
                output.push(tri);
                continue;
            }
            let mut inside = tri.to_vec();
            for plane in b.planes() {
                {
                    if inside.is_empty() {
                        break;
                    }
                    let distance = |p: [f64; 6]| {
                        let q = b.local([p[0], p[1], p[2]]);
                        plane[3] + (0..3).map(|k| plane[k] * q[k]).sum::<f64>()
                    };
                    let cut = |keep: bool| {
                        let mut out = Vec::new();
                        let mut a = *inside.last().unwrap();
                        let mut da = distance(a);
                        for &c in &inside {
                            let dc = distance(c);
                            let ka = if keep { da >= 0. } else { da < 0. };
                            let kc = if keep { dc >= 0. } else { dc < 0. };
                            if ka != kc {
                                let t = da / (da - dc);
                                out.push(std::array::from_fn(|k| a[k] + t * (c[k] - a[k])));
                            }
                            if kc {
                                out.push(c);
                            }
                            a = c;
                            da = dc;
                        }
                        out
                    };
                    let outside = cut(false);
                    for k in 1..outside.len().saturating_sub(1) {
                        let t = [outside[0], outside[k], outside[k + 1]];
                        let a: V = std::array::from_fn(|j| t[1][j] - t[0][j]);
                        let b: V = std::array::from_fn(|j| t[2][j] - t[0][j]);
                        let n = [
                            a[1] * b[2] - a[2] * b[1],
                            a[2] * b[0] - a[0] * b[2],
                            a[0] * b[1] - a[1] * b[0],
                        ];
                        if n.iter().map(|x| x * x).sum::<f64>() > 1e-18 {
                            output.push(t);
                        }
                    }
                    inside = cut(true);
                }
            }
        }
        triangles = output;
    }
    triangles
}

/// Newly exposed boundary of the surrounding, unactivated ground. Occupancy
/// comes from the original geological field, so a cave entrance is not sealed.
pub(crate) fn shell(
    edits: &[Value],
    field: Option<&super::underground::Field>,
) -> Vec<super::tool_cut::Triangle> {
    let mut field = field.cloned();
    if let Some(f) = &mut field {
        f.set_material_domains(&[]);
    }
    let mut out = Vec::new();
    let domains = from_edits(edits);
    let cell_key = |p: V| p.map(|x| (x / CELL_SIDE).round() as i64);
    let cells: std::collections::HashSet<_> = domains
        .iter()
        .filter(|d| d.mined && d.tetra.is_none())
        .map(|d| cell_key(d.origin))
        .collect();
    let physical: Vec<_> = domains.iter().filter(|d| !d.mined).collect();
    for (index, d) in domains.iter().enumerate() {
        if let Some(v) = d.tetra {
            let mut faces = Vec::new();
            for [a, b, c, other] in [[0, 1, 2, 3], [0, 3, 1, 2], [1, 3, 2, 0], [2, 3, 0, 1]] {
                let mut face = [v[a], v[b], v[c]];
                // Shared tile interfaces disappear as whole faces. Comparing
                // vertices also avoids roundoff slivers at planet coordinates.
                let world_face = face.map(|p| d.world(p));
                if domains.iter().enumerate().any(|(i, other)| {
                    i != index
                        && other.tetra.is_some_and(|vertices| {
                            let vertices = vertices.map(|p| other.world(p));
                            world_face.iter().all(|p| {
                                vertices
                                    .iter()
                                    .any(|q| (0..3).all(|k| (p[k] - q[k]).abs() < 1e-7))
                            })
                        })
                }) {
                    continue;
                }
                // Inward-facing cavity triangles (world basis is right handed).
                let u: V = std::array::from_fn(|k| face[1][k] - face[0][k]);
                let w: V = std::array::from_fn(|k| face[2][k] - face[0][k]);
                let n = [
                    u[1] * w[2] - u[2] * w[1],
                    u[2] * w[0] - u[0] * w[2],
                    u[0] * w[1] - u[1] * w[0],
                ];
                if (0..3)
                    .map(|k| n[k] * (v[other][k] - face[0][k]))
                    .sum::<f64>()
                    < 0.
                {
                    face.swap(1, 2);
                }
                let sample = |i: usize, j: usize| {
                    d.world(std::array::from_fn(|k| {
                        face[0][k]
                            + (face[1][k] - face[0][k]) * i as f64 / 12.
                            + (face[2][k] - face[0][k]) * j as f64 / 12.
                    }))
                };
                for i in 0..12 {
                    for j in 0..12 - i {
                        let mut pieces = vec![[sample(i, j), sample(i + 1, j), sample(i, j + 1)]];
                        if i + j + 1 < 12 {
                            pieces.push([sample(i + 1, j), sample(i + 1, j + 1), sample(i, j + 1)]);
                        }
                        for t in pieces {
                            let signed = |p: V| {
                                let r = p.iter().map(|x| x * x).sum::<f64>().sqrt();
                                let h = super::height(p.map(|x| x / r));
                                field
                                    .as_ref()
                                    .map_or(r - super::RADIUS - h, |f| f.distance(p, h))
                            };
                            let mut poly = Vec::new();
                            for k in 0..3 {
                                let a = t[(k + 2) % 3];
                                let b = t[k];
                                let da = signed(a);
                                let db = signed(b);
                                if (da <= 0.) != (db <= 0.) {
                                    let f = da / (da - db);
                                    poly.push(std::array::from_fn(|k| a[k] + f * (b[k] - a[k])));
                                }
                                if db <= 0. {
                                    poly.push(b);
                                }
                            }
                            for k in 2..poly.len() {
                                faces.push([poly[0], poly[k - 1], poly[k]].map(|p| {
                                    let color =
                                        crate::physics::matter::world_field::substance(p).color();
                                    [
                                        p[0],
                                        p[1],
                                        p[2],
                                        color[0] as f64,
                                        color[1] as f64,
                                        color[2] as f64,
                                    ]
                                }));
                            }
                        }
                    }
                }
            }
            let others: Vec<_> = domains
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, d)| d.clone())
                .collect();
            out.extend(clip(faces, &others));
            continue;
        }
        let world = |p: V| d.world(p);
        for axis in 0..3 {
            for upper in [false, true] {
                let mined = d.mined;
                if !mined && axis == 1 && upper {
                    continue;
                }
                if mined {
                    let mut neighbor = cell_key(d.origin);
                    neighbor[axis] += if upper { 1 } else { -1 };
                    if cells.contains(&neighbor) {
                        continue;
                    }
                }
                let a = (axis + 1) % 3;
                let b = (axis + 2) % 3;
                let na = ((d.max[a] - d.min[a]) / 0.08).ceil().max(1.) as usize;
                let nb = ((d.max[b] - d.min[b]) / 0.08).ceil().max(1.) as usize;
                for i in 0..na {
                    for j in 0..nb {
                        let p: [[f64; 3]; 4] = std::array::from_fn(|k| {
                            let mut p = d.min;
                            p[axis] = if upper { d.max[axis] } else { d.min[axis] };
                            p[a] += (d.max[a] - d.min[a])
                                * (i + usize::from(k == 1 || k == 2)) as f64
                                / na as f64;
                            p[b] += (d.max[b] - d.min[b]) * (j + usize::from(k >= 2)) as f64
                                / nb as f64;
                            world(p)
                        });
                        for ids in [[0, 1, 2], [0, 2, 3]] {
                            let mut t = ids.map(|i| p[i]);
                            if upper {
                                t.swap(1, 2);
                            }
                            let center: V =
                                std::array::from_fn(|k| t.iter().map(|v| v[k]).sum::<f64>() / 3.);
                            // An adjacent refined region owns this interface.
                            // Do not leave an immovable wall inside its matter.
                            if if mined {
                                physical.iter().any(|d| d.distance(center) < -0.001)
                            } else {
                                domains
                                    .iter()
                                    .enumerate()
                                    .any(|(i, d)| i != index && d.distance(center) < -0.001)
                            } {
                                continue;
                            }
                            let signed = |p: V| {
                                let r = p.iter().map(|v| v * v).sum::<f64>().sqrt();
                                let normal = p.map(|v| v / r);
                                let h = super::height(normal);
                                let h = if mined {
                                    h
                                } else {
                                    super::tool_cut::height(normal, h, edits, false)
                                };
                                field
                                    .as_ref()
                                    .map_or(r - super::RADIUS - h, |f| f.distance(p, h))
                            };
                            let color =
                                crate::physics::matter::world_field::substance(center).color();
                            let vertex = |p: V| {
                                [
                                    p[0],
                                    p[1],
                                    p[2],
                                    color[0] as f64,
                                    color[1] as f64,
                                    color[2] as f64,
                                ]
                            };
                            if !mined {
                                if signed(center) <= 0.005 {
                                    out.push(t.map(vertex));
                                }
                                continue;
                            }
                            // Clip at the actual ground/cave interface. Dropping
                            // triangles by centroid leaves a sawtooth at the lip.
                            let distances = t.map(signed);
                            let mut polygon = [[0.; 3]; 4];
                            let mut count = 0;
                            for k in 0..3 {
                                let j = (k + 2) % 3;
                                if (distances[j] <= 0.) != (distances[k] <= 0.) {
                                    let f = distances[j] / (distances[j] - distances[k]);
                                    polygon[count] =
                                        std::array::from_fn(|a| t[j][a] + f * (t[k][a] - t[j][a]));
                                    count += 1;
                                }
                                if distances[k] <= 0. {
                                    polygon[count] = t[k];
                                    count += 1;
                                }
                            }
                            for k in 2..count {
                                out.push([polygon[0], polygon[k - 1], polygon[k]].map(vertex));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod faceted_cut_checks {
    use super::*;
    fn edit() -> Value {
        serde_json::json!({"mined":true,"domain":{"origin":[0.,100.,0.],"axes":[[1,0,0],[0,1,0],[0,0,1]],"min":[-1,-1,-1],"max":[1,1,1],"tetra":[[-1,0.2,-1],[1,0.2,-1],[0,0.2,1],[0,-1,0]]}})
    }
    #[test]
    fn pointed_cut_preserves_box_corners_and_clips_surface() {
        let d = Domain::parse(&edit()).unwrap();
        assert!(d.distance(d.world([0., 0., 0.])) < 0.);
        assert!(d.distance(d.world([0.8, -0.8, 0.8])) > 0.);
        assert!(d.distance(d.world([0., -1.01, 0.])) > 0.);
        let tri = [
            [-2., 100., -2., 1., 1., 1.],
            [2., 100., -2., 1., 1., 1.],
            [0., 100., 2., 1., 1., 1.],
        ];
        let clipped = clip(vec![tri], &[d.clone()]);
        assert!(!clipped.is_empty());
        for t in clipped {
            let c = std::array::from_fn(|k| t.iter().map(|p| p[k]).sum::<f64>() / 3.);
            assert!(d.distance(c) > -1e-7);
        }
    }
    #[test]
    fn rejects_flat_or_nonfinite_pyramids_and_retains_legacy_boxes() {
        let mut e = edit();
        e["domain"]["tetra"][3] = serde_json::json!([0, 0.2, 0]);
        assert!(Domain::parse(&e).is_none());
        e["domain"].as_object_mut().unwrap().remove("tetra");
        let d = Domain::parse(&e).unwrap();
        assert!(d.distance(d.world([0.; 3])) < 0.);
    }
}

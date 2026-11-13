//! Blade-footprint cuts shared by visible terrain, height queries and contact meshes.
use serde_json::Value;
type P = [f64; 3];
pub(crate) type Vertex = [f64; 6];
pub(crate) type Triangle = [Vertex; 3];
fn dot(a: P, b: P) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn sub(a: P, b: P) -> P {
    std::array::from_fn(|i| a[i] - b[i])
}
fn point(v: &Value) -> P {
    std::array::from_fn(|i| v[i].as_f64().unwrap())
}
pub(crate) fn valid_edits(v: &Value) -> bool {
    let Some(edits) = v.as_array().filter(|a| a.len() <= 4096) else {
        return false;
    };
    edits.iter().all(|e| {
        if !e["domain"].is_null() {return super::material_domain::Domain::parse(e).is_some();}
        let vector = |v: &Value| {
            v.as_array().is_some_and(|a| {
                a.len() == 3 && a.iter().all(|n| n.as_f64().is_some_and(f64::is_finite))
            })
        };
        if !vector(&e["normal"])
            || (dot(point(&e["normal"]), point(&e["normal"])) - 1.).abs() > 1e-6
            || !e["radius"]
                .as_f64()
                .is_some_and(|x| (0.05..=4.).contains(&x))
            || !e["depth"]
                .as_f64()
                .is_some_and(|x| (0.005..=2.).contains(&x))
        {
            return false;
        }
        if e["blade"].is_null() {
            return e.as_object().is_some_and(|o| o.len() == 3);
        }
        let b = &e["blade"];
        if !e.as_object().is_some_and(|o| o.len() == 4)
            || !b.as_object().is_some_and(|o| o.len() == 5)
            || !vector(&b["right"])
            || !vector(&b["forward"])
        {
            return false;
        }
        let n = point(&e["normal"]);
        let r = point(&b["right"]);
        let f = point(&b["forward"]);
        if (dot(r, r) - 1.).abs() > 1e-6
            || (dot(f, f) - 1.).abs() > 1e-6
            || dot(r, n).abs() > 1e-6
            || dot(f, n).abs() > 1e-6
            || dot(r, f).abs() > 1e-6
        {
            return false;
        }
        let Some(outline) = b["outline"]
            .as_array()
            .filter(|a| (3..=32).contains(&a.len()))
        else {
            return false;
        };
        if !b["bottom"]
            .as_f64()
            .is_some_and(|x| x.is_finite() && x.abs() < 100_000.)
            || !b["top"].as_array().is_some_and(|a| {
                a.len() == outline.len()
                    && a.iter().all(|x| {
                        x.as_f64()
                            .is_some_and(|x| x.is_finite() && x.abs() < 100_000.)
                    })
            })
        {
            return false;
        }
        outline.iter().all(|v| {
            v.as_array().is_some_and(|a| {
                a.len() == 2
                    && a.iter()
                        .all(|x| x.as_f64().is_some_and(|x| x.is_finite() && x.abs() <= 1.))
            })
        })
    })
}
fn xy(p: P, e: &Value) -> [f64; 2] {
    let n = point(&e["normal"]);
    let d = sub(p, n.map(|x| x * super::RADIUS));
    [
        dot(d, point(&e["blade"]["right"])),
        dot(d, point(&e["blade"]["forward"])),
    ]
}
fn side(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
fn outline(e: &Value) -> Vec<[f64; 2]> {
    e["blade"]["outline"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()])
        .collect()
}
pub(crate) fn inside(n: P, e: &Value) -> bool {
    if !e["domain"].is_null() || e["blade"].is_null() {
        return false;
    }
    let p = xy(n.map(|x| x * super::RADIUS), e);
    let hull = outline(e);
    (0..hull.len()).all(|i| side(p, hull[i], hull[(i + 1) % hull.len()]) >= -1e-8)
}
pub(crate) fn height(n: P, mut h: f64, edits: &[Value], blades: bool) -> f64 {
    for e in edits {
        // Cave/body queries subtract mined convex volumes in three dimensions. Lowering
        // the geological height here as well would erase tunnel ceilings.
        if e["mined"] == true { continue; }
        if let Some(d)=super::material_domain::Domain::parse(e) {
            if !blades{continue;}
            let p=n.map(|x|x*(super::RADIUS+h));let q=d.local(p);
            if q[0]>=d.min[0]&&q[0]<=d.max[0]&&q[2]>=d.min[2]&&q[2]<=d.max[2]&&q[1]>=d.min[1]&&q[1]<=d.max[1]+0.01 {h-=q[1]-d.min[1];}
            continue;
        }
        if !e["blade"].is_null() {
            if blades && inside(n, e) {
                h = h.min(e["blade"]["bottom"].as_f64().unwrap());
            }
            continue;
        }
        let delta = sub(n, point(&e["normal"]));
        let d2 = dot(delta, delta) * super::RADIUS.powi(2);
        let radius = e["radius"].as_f64().unwrap();
        if d2 < radius * radius {
            h -= e["depth"].as_f64().unwrap() * (1. - d2 / (radius * radius));
        }
    }
    h
}
fn clip(poly: &[Vertex], e: &Value, a: [f64; 2], b: [f64; 2], keep_inside: bool) -> Vec<Vertex> {
    let mut out = Vec::new();
    if poly.is_empty() {
        return out;
    }
    let distance = |p: Vertex| {
        let d = side(xy([p[0], p[1], p[2]], e), a, b);
        if keep_inside { d } else { -d }
    };
    let mut previous = *poly.last().unwrap();
    let mut old = distance(previous);
    for &current in poly {
        let d = distance(current);
        if (d >= -1e-9) != (old >= -1e-9) {
            let t = old / (old - d);
            out.push(std::array::from_fn(|i| {
                previous[i] + (current[i] - previous[i]) * t
            }));
        }
        if d >= -1e-9 {
            out.push(current);
        }
        previous = current;
        old = d;
    }
    out
}
fn fan(poly: Vec<Vertex>, out: &mut Vec<Triangle>) {
    for i in 1..poly.len().saturating_sub(1) {
        let a = [poly[0][0], poly[0][1], poly[0][2]];
        let b = sub([poly[i][0], poly[i][1], poly[i][2]], a);
        let c = sub([poly[i + 1][0], poly[i + 1][1], poly[i + 1][2]], a);
        let area = [
            b[1] * c[2] - b[2] * c[1],
            b[2] * c[0] - b[0] * c[2],
            b[0] * c[1] - b[1] * c[0],
        ];
        if dot(area, area) > 1e-18 {
            out.push([poly[0], poly[i], poly[i + 1]]);
        }
    }
}
/// Subtract each convex blade prism by splitting triangles at its actual edges.
/// Outside triangles retain their surface. Inside triangles become the cut floor;
/// walls join the old surface to that floor. No grid/voxel quantization is used.
pub(crate) fn mesh(mut triangles: Vec<Triangle>, edits: &[Value]) -> Vec<Triangle> {
    for e in edits.iter().filter(|e| e["domain"].is_null() && !e["blade"].is_null()) {
        let n = point(&e["normal"]);
        let right = point(&e["blade"]["right"]);
        let forward = point(&e["blade"]["forward"]);
        let bottom = e["blade"]["bottom"].as_f64().unwrap();
        let hull = outline(e);
        let radius = e["radius"].as_f64().unwrap();
        let mut next = Vec::with_capacity(triangles.len() + hull.len() * 4);
        for t in triangles {
            let coords = t.map(|v| xy([v[0], v[1], v[2]], e));
            if (0..2).any(|axis| {
                coords.iter().all(|p| p[axis] > radius) || coords.iter().all(|p| p[axis] < -radius)
            }) {
                next.push(t);
                continue;
            }
            let mut inner = t.to_vec();
            for i in 0..hull.len() {
                let a = hull[i];
                let b = hull[(i + 1) % hull.len()];
                let outside = clip(&inner, e, a, b, false);
                fan(outside, &mut next);
                inner = clip(&inner, e, a, b, true);
                if inner.is_empty() {
                    break;
                }
            }
            for v in &mut inner {
                let above = dot([v[0], v[1], v[2]], n) - super::RADIUS - bottom;
                if above > 0. {
                    for j in 0..3 {
                        v[j] -= n[j] * above;
                    }
                    v[3] = 0.32;
                    v[4] = 0.20;
                    v[5] = 0.10;
                }
            }
            fan(inner, &mut next);
        }
        let vertex = |i: usize, h: f64| {
            let p: P = std::array::from_fn(|j| {
                n[j] * (super::RADIUS + h) + right[j] * hull[i][0] + forward[j] * hull[i][1]
            });
            [p[0], p[1], p[2], 0.34, 0.22, 0.11]
        };
        for i in 0..hull.len() {
            let j = (i + 1) % hull.len();
            let top_i = e["blade"]["top"][i].as_f64().unwrap().max(bottom);
            let top_j = e["blade"]["top"][j].as_f64().unwrap().max(bottom);
            fan(
                vec![
                    vertex(i, top_i),
                    vertex(j, top_j),
                    vertex(j, bottom),
                    vertex(i, bottom),
                ],
                &mut next,
            );
        }
        triangles = next;
    }
    triangles
}
pub(crate) fn hull(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup_by(|a, b| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() < 1e-7);
    let mut h = Vec::new();
    for &q in &p {
        while h.len() >= 2 && side(q, h[h.len() - 2], h[h.len() - 1]) <= 1e-12 {
            h.pop();
        }
        h.push(q);
    }
    let lower = h.len();
    for &q in p.iter().rev().skip(1) {
        while h.len() > lower && side(q, h[h.len() - 2], h[h.len() - 1]) <= 1e-12 {
            h.pop();
        }
        h.push(q);
    }
    h.pop();
    h
}

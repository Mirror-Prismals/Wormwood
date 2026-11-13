//! A lazy, world-fixed tiling of connected solid pieces, independent of tools.
//! Six consistently oriented tetrahedra fill each lattice cell. Every lattice
//! vertex is shared, including its small deterministic displacement, so adjacent
//! pieces have identical triangular interfaces. Geological occupancy trims the
//! pieces at the surface and caves; it does not choose their topology.
use super::material_domain::{CELL_SIDE, Domain};
use serde_json::{Value, json};
type V = [f64; 3];
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn vertex(key: [i64; 3]) -> V {
    let hash = |axis: usize| {
        let mut h = 0x9e3779b97f4a7c15u64.wrapping_mul(axis as u64 + 1);
        for x in key {
            h ^= x as u64;
            h = (h ^ (h >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        }
        h ^= h >> 27;
        (h & 0xffff) as f64 / 65535. - 0.5
    };
    std::array::from_fn(|k| (key[k] as f64 + 0.10 * hash(k)) * CELL_SIDE)
}

fn piece(cell: [i64; 3], part: usize) -> Value {
    let mut corner = cell;
    let mut vertices = [vertex(corner); 4];
    for (i, axis) in ORDERS[part].iter().enumerate() {
        corner[*axis] += 1;
        vertices[i + 1] = vertex(corner);
    }
    let origin = cell.map(|k| k as f64 * CELL_SIDE);
    let local = vertices.map(|v| std::array::from_fn::<_, 3, _>(|k| v[k] - origin[k]));
    let min: V = std::array::from_fn(|k| local.iter().map(|v| v[k]).fold(f64::INFINITY, f64::min));
    let max: V =
        std::array::from_fn(|k| local.iter().map(|v| v[k]).fold(f64::NEG_INFINITY, f64::max));
    json!({"mined":true,"tile":{"cell":cell,"part":part},"domain":{"origin":origin,
        "axes":[[1,0,0],[0,1,0],[0,0,1]],"min":min,"max":max,"tetra":local}})
}

/// The smallest stable tile identity wins on an exactly shared boundary.
/// Only a bounded neighbourhood is evaluated; no planet-sized allocation.
pub(crate) fn at(p: V) -> Option<Value> {
    if p.iter().any(|x| !x.is_finite() || x.abs() > 1e9) {
        return None;
    }
    let base = p.map(|x| (x / CELL_SIDE).floor() as i64);
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let cell = [base[0] + x, base[1] + y, base[2] + z];
                for part in 0..6 {
                    let e = piece(cell, part);
                    if Domain::parse(&e)?.distance(p) <= 1e-9 {
                        return Some(e);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn volume(v: [V; 4]) -> f64 {
        let a: V = std::array::from_fn(|k| v[1][k] - v[0][k]);
        let b: V = std::array::from_fn(|k| v[2][k] - v[0][k]);
        let c: V = std::array::from_fn(|k| v[3][k] - v[0][k]);
        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.
    }
    #[test]
    fn pieces_have_shared_faces_and_positive_volume() {
        let mut faces = std::collections::BTreeMap::new();
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    for part in 0..6 {
                        let d = Domain::parse(&piece([x, y, z], part)).unwrap();
                        let v = d.tetra.unwrap().map(|p| d.world(p));
                        assert!(volume(v).abs() > CELL_SIDE.powi(3) / 12.);
                        for ids in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
                            let mut face = ids.map(|i| v[i].map(|x| (x * 1e8).round() as i64));
                            face.sort();
                            *faces.entry(face).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        assert!(faces.values().all(|n| *n == 1 || *n == 2));
        assert!(faces.values().filter(|n| **n == 2).count() > 200);
    }
    #[test]
    fn tiled_ownership_has_no_gaps_or_overlapping_interiors() {
        for n in 0..200 {
            let p = [
                ((n * 71 % 211) as f64 / 211. - 0.5) * 1.2,
                ((n * 37 % 223) as f64 / 223. - 0.5) * 1.2,
                ((n * 97 % 227) as f64 / 227. - 0.5) * 1.2,
            ];
            let e = at(p).expect("every point belongs to the tiling");
            let d = Domain::parse(&e).unwrap();
            assert!(d.distance(p) < 1e-8);
            let cell: [i64; 3] = serde_json::from_value(e["tile"]["cell"].clone()).unwrap();
            let mut interiors = 0;
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        for part in 0..6 {
                            let d = Domain::parse(&piece(
                                [cell[0] + x, cell[1] + y, cell[2] + z],
                                part,
                            ))
                            .unwrap();
                            interiors += usize::from(d.distance(p) < -1e-8);
                        }
                    }
                }
            }
            assert!(interiors <= 1);
            assert_eq!(at(p).unwrap()["tile"], e["tile"]);
        }
    }
    #[test]
    fn adjoining_removed_pieces_leave_no_internal_face() {
        let cell = [0, (super::super::RADIUS / CELL_SIDE).floor() as i64 - 4, 0];
        let a = piece(cell, 0);
        let b = piece(cell, 1);
        let da = Domain::parse(&a).unwrap();
        let db = Domain::parse(&b).unwrap();
        let common: Vec<_> = da
            .tetra
            .unwrap()
            .map(|p| da.world(p))
            .into_iter()
            .filter(|p| {
                db.tetra
                    .unwrap()
                    .map(|q| db.world(q))
                    .iter()
                    .any(|q| (0..3).all(|k| (p[k] - q[k]).abs() < 1e-7))
            })
            .collect();
        assert_eq!(common.len(), 3);
        let u: V = std::array::from_fn(|k| common[1][k] - common[0][k]);
        let v: V = std::array::from_fn(|k| common[2][k] - common[0][k]);
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let shell = super::super::material_domain::shell(&[a, b], None);
        assert!(!shell.is_empty());
        assert!(shell.iter().all(|t| !t.iter().all(|p| {
            (0..3)
                .map(|k| n[k] * (p[k] - common[0][k]))
                .sum::<f64>()
                .abs()
                < 1e-8
        })));
    }
    #[test]
    fn planet_scale_coordinates_keep_piece_identity() {
        for p in [[0.125, 6_371_000.24, -0.315], [-6_371_000.2, 0.017, 1.02]] {
            let e = at(p).unwrap();
            let d = Domain::parse(&e).unwrap();
            assert!(d.distance(p) < 1e-8);
            let center = d.tetra.unwrap().map(|v| d.world(v));
            let center = std::array::from_fn(|k| center.iter().map(|p| p[k]).sum::<f64>() / 4.);
            assert_eq!(at(center).unwrap()["tile"], e["tile"]);
        }
    }
}

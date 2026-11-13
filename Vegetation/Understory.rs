//! Blender-authored ground flora, using the existing lit/shadowed vegetation ABI.
//! Stable cube-face colonies; visual decoration only, not tree/perch/wood state.
use super::{State, forest::View, math::*, packet::Geometry};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::OnceLock;
#[derive(Deserialize)]
struct Lod {
    vertices: Vec<[f32; 16]>,
    indices: Vec<u32>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    species: String,
    lods: Vec<Lod>,
}
fn assets() -> &'static [Asset] {
    static DATA: OnceLock<Vec<Asset>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../Art/Understory/runtime.json"))
            .expect("validated Blender understory export")
    })
}
#[derive(Clone)]
struct Plant {
    root: V,
    up: V,
    seed: u64,
    species: usize,
}
#[derive(Default)]
pub(crate) struct Understory {
    anchor: Option<[i64; 3]>,
    plants: Vec<Plant>,
}
impl Understory {
    fn refresh(&mut self, eye: V) {
        let anchor = eye.map(|v| (v / 12.).floor() as i64);
        if self.anchor == Some(anchor) {
            return;
        }
        self.anchor = Some(anchor);
        self.plants.clear();
        let normal = unit(eye);
        let maximum = normal.iter().map(|v| v.abs()).fold(0., f64::max);
        // Fixed world grid, including face seam ownership. The extra margin
        // exceeds an anchor's diagonal so travel cannot reveal stale edges.
        for axis in 0..3 {
            if normal[axis].abs() < maximum - 240. / crate::terrain::RADIUS {
                continue;
            }
            let u = (axis + 1) % 3;
            let v = (axis + 2) % 3;
            let cx = (normal[u] / normal[axis].abs() * crate::terrain::RADIUS / 6.).floor() as i64;
            let cz = (normal[v] / normal[axis].abs() * crate::terrain::RADIUS / 6.).floor() as i64;
            for x in cx - 17..=cx + 17 {
                for z in cz - 17..=cz + 17 {
                    let seed = hash(
                        (x as u64).wrapping_mul(0x632be59bd9b4e019)
                            ^ (z as u64).wrapping_mul(0x9e3779b97f4a7c15)
                            ^ ((axis as u64 * 2 + u64::from(normal[axis] < 0.)) << 56)
                            ^ 0x6665726e,
                    );
                    if random(seed, 0) > 0.86 {
                        continue;
                    }
                    let mut raw = [0.; 3];
                    raw[axis] = normal[axis].signum();
                    raw[u] = (x as f64 + 0.2 + 0.6 * random(seed, 1)) * 6. / crate::terrain::RADIUS;
                    raw[v] = (z as f64 + 0.2 + 0.6 * random(seed, 2)) * 6. / crate::terrain::RADIUS;
                    if raw[u].abs() >= 1. || raw[v].abs() >= 1. {
                        continue;
                    }
                    let n = unit(raw);
                    if length(mul(sub(n, normal), crate::terrain::RADIUS)) > 64. {
                        continue;
                    }
                    if crate::terrain::woodland::density(n) < 0.25 {
                        continue;
                    }
                    let s = crate::terrain::sample(n);
                    if s.soil_depth < 0.22 || crate::terrain::drainage::water(n).is_some() {
                        continue;
                    }
                    // One species per colony gives fern patches, shrub edges and
                    // broad-leaf colonies rather than a uniform three-plant mixture.
                    let choice = random(seed, 3);
                    let mut species = if s.moisture > 0.42 && choice < 0.62 {
                        0
                    } else if s.soil_depth > 0.48 && choice > 0.64 {
                        2
                    } else {
                        1
                    };
                    // Additional shrub, herb, sedge and mineral colonies. Rocks remain
                    // decorative: no fake terrain collision or material transfer.
                    let diversity = random(seed, 23);
                    if random(seed, 0) > 0.58 && diversity < 0.16 { species = 3; }
                    else if random(seed, 0) > 0.58 && diversity < 0.32 { species = 4; }
                    else if random(seed, 0) > 0.58 && diversity < 0.50 { species = 5; }
                    else if random(seed, 0) > 0.58 && diversity < 0.60 { species = 6; }
                    else if random(seed, 0) > 0.58 && diversity < 0.69 { species = 7; }
                    else if random(seed, 0) > 0.58 && diversity < 0.77 { species = 8; }
                    let (e, north) = basis(n);
                    for j in 0..(if species == 1 || species == 3 { 2 } else { 6 }) {
                        let id = hash(seed.wrapping_add(j));
                        let angle = random(id, 5) * std::f64::consts::TAU;
                        let radius = random(id, 6) * 1.8;
                        let up = unit(add(
                            mul(n, crate::terrain::RADIUS),
                            add(
                                mul(e, angle.cos() * radius),
                                mul(north, angle.sin() * radius),
                            ),
                        ));
                        if crate::terrain::drainage::water(up).is_some() {
                            continue;
                        }
                        let root = mul(up, crate::terrain::RADIUS + crate::terrain::height(up));
                        self.plants.push(Plant {
                            root,
                            up,
                            seed: id,
                            species,
                        });
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "ios"))]
    pub(crate) fn review_root(&mut self, eye: V, species: usize) -> Option<V> {
        self.refresh(eye);
        eprintln!("understory review: requested={species}, colonies={:?}, moisture={}", [0,1,2].map(|s|self.plants.iter().filter(|p|p.species==s).count()), crate::terrain::sample(unit(eye)).moisture);
        self.plants
            .iter()
            .find(|p| p.species == species)
            .map(|p| p.root)
    }

    pub(crate) fn append(
        &mut self,
        state: &State,
        input: &Value,
        view: &View,
        range: f64,
        budget: usize,
        out: &mut Geometry,
    ) {
        self.refresh(view.eye);
        let day = input["calendar"]["day_of_year"].as_f64().unwrap_or(180.);
        let start = out.indices.len();
        let mut instances = Vec::new();
        let mut visible: Vec<_> = self
            .plants
            .iter()
            .filter(|p| {
                length(sub(p.root, view.eye)) < range.min(40.)
                    && view.visible(add(p.root, mul(p.up, 0.7)), 1.8)
                    && !state.clearings.iter().any(|(c, r)| {
                        let d = sub(p.root, *c);
                        length(sub(d, mul(p.up, dot(d, p.up)))) < *r + 1.0
                    })
            })
            .collect();
        visible.sort_by(|a, b| {
            length(sub(a.root, view.eye))
                .total_cmp(&length(sub(b.root, view.eye)))
                .then(a.seed.cmp(&b.seed))
        });
        for p in visible {
            // Fern and laurel are evergreen studies. Mayapple disappears in
            // late summer/winter; flowering laurel is a fixed first asset study.
            if (p.species == 2 && !(85. ..=230.).contains(&day))
                || ((p.species == 3 || p.species == 4) && !(85. ..=300.).contains(&day)) {
                continue;
            }
            let distance = length(sub(p.root, view.eye));
            let level = if distance < 10. {
                0
            } else if distance < 23. {
                1
            } else {
                2
            };
            let asset = &assets()[p.species * 3 + (p.seed % 3) as usize];
            let mesh = &asset.lods[level];
            if (out.indices.len() - start + mesh.indices.len()) / 3 > budget {
                continue;
            }
            let scale = 0.78 + 0.30 * random(p.seed, 9);
            let angle = random(p.seed, 10) * std::f64::consts::TAU;
            let (e, n) = basis(p.up);
            let right = add(mul(e, angle.cos()), mul(n, angle.sin()));
            let forward = add(mul(e, angle.sin()), mul(n, -angle.cos()));
            let direction = |v: V| add(add(mul(right, v[0]), mul(p.up, v[1])), mul(forward, v[2]));
            let offset = out.vertices.len() as u32;
            let begin = out.indices.len();
            for v in &mesh.vertices {
                let pos = add(
                    sub(p.root, view.eye),
                    direction([
                        v[0] as f64 * scale,
                        v[1] as f64 * scale,
                        v[2] as f64 * scale,
                    ]),
                );
                let norm = direction([v[3] as f64, v[4] as f64, v[5] as f64]);
                let mut vertex = *v;
                for i in 0..3 {
                    vertex[i] = pos[i] as f32;
                    vertex[i + 3] = norm[i] as f32;
                }
                out.vertices.push(vertex);
            }
            out.indices.extend(mesh.indices.iter().map(|i| offset + i));
            instances.push(json!({"species":asset.species,"asset":asset.name,"seed":p.seed,"root":p.root,"lod":level,"index_start":begin,"index_count":mesh.indices.len()}));
        }
        out.metadata["understory_triangles"] = json!((out.indices.len() - start) / 3);
        out.metadata["understory_instances"] = json!(instances);
        out.metadata["understory_cached_plants"] = json!(self.plants.len());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_understory_meshes_are_bounded_and_keep_solid_materials() {
        assert_eq!(assets().len(), 27);
        for a in assets() {
            assert_eq!(a.lods.len(), 3);
            let mut previous = usize::MAX;
            for m in &a.lods {
                assert!(!m.indices.is_empty());
                assert!(m.indices.len() / 3 <= previous);
                previous = m.indices.len() / 3;
                assert!(previous <= 700);
                assert!(m.indices.iter().all(|i| (*i as usize) < m.vertices.len()));
                assert!(m.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())
                    && v[11] == 1.
                    && v[6] == 0.5
                    && v[7] == 0.5));
            }
        }
    }
    #[test]
    fn colonies_are_stable_budgeted_and_respect_clearings() {
        let eye = crate::vegetation::forest_test_position();
        let input = json!({"position":eye,"forward":[0.,0.,-1.],"calendar":{"day_of_year":180},"aspect":1.5});
        let view = View::from_input(&input).unwrap();
        let mut cache = Understory::default();
        let mut state = State::default();
        let packet = || Geometry {
            origin: eye,
            revision: 0,
            vertices: vec![],
            indices: vec![],
            metadata: json!({}),
        };
        let mut a = packet();
        cache.append(&state, &input, &view, 40., 4096, &mut a);
        assert!(!a.indices.is_empty(), "eastern woodland should contain flora");
        assert!(a.indices.len() / 3 <= 4096);
        assert!(cache.plants.iter().any(|p| p.species==0), "fern colony expected in eastern woodland");
        assert!(cache.plants.iter().any(|p| p.species==1), "laurel colony expected");
        assert!(cache.plants.iter().any(|p| p.species==2), "mayapple colony expected");
        let mut b = packet();
        cache.append(&state, &input, &view, 40., 4096, &mut b);
        assert_eq!(a.vertices, b.vertices);
        assert_eq!(a.indices, b.indices);
        state.clearings.push((eye, 100.));
        let mut c = packet();
        cache.append(&state, &input, &view, 40., 4096, &mut c);
        assert!(c.indices.is_empty());
        let mut d = packet();
        cache.append(&State::default(), &input, &view, 40., 0, &mut d);
        assert!(d.indices.is_empty());
    }
}
